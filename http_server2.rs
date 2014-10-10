#![crate_name="http_server2"]
#![crate_type="lib"]
#![feature(macro_rules, phase)]
#![allow(experimental)]

#[phase(plugin, link)]
extern crate log;

extern crate time;

extern crate green;

use std::io::{TcpListener, TcpStream, BufferedReader, BufferedWriter, IoResult, Reader, Buffer, Acceptor, Listener};
use std::collections::HashMap;
use std::task::{TaskBuilder};
use std::sync::Arc;
use time::now;

use green::{SchedPool, PoolConfig, GreenTaskBuilder};


#[deriving(Show)]
pub enum HTTPMethod {
    GET, POST, HEAD, NoMethod
}


#[deriving(Show)]
pub enum HTTPResponseCode {
    HTTP200 = 200,
    HTTP301 = 301, HTTP302 = 302,
    HTTP400 = 400, HTTP401 = 401, HTTP403 = 403, HTTP404 = 404,
    HTTP500 = 500,
    HTTPERROR = 0,
}


pub type HTTPHeaders = HashMap<Vec<u8>, Vec<u8>>;


pub trait HTTPRequestHandler
    <'req, R: Reader + Send + Sized, W: Writer + Send + Sized>
    : Send + Sized
{
    fn handle(
        &self,
        method: HTTPMethod,
        path: &Vec<u8>,
        headers: &HTTPHeaders,
        mut stream: BufferedReader<R>)
        -> IoResult<Option<(HTTPResponseCode,
                            Box<HTTPHeaders>,
                            Box<HTTPResponseWriter<W> + 'req>)>>;
}


pub trait HTTPResponseWriter<W: Writer + Send + Sized> {
    fn get_content_length(&self) -> Option<u64>;
    fn get_content_type(&self) -> String;
    fn write_data(&self, mut stream: BufferedWriter<W>) -> IoResult<()>;
}


fn update_response_headers<W: Writer + Send + Sized>
    (writer: &HTTPResponseWriter<W>,
     headers: &mut HTTPHeaders)
{
    let content_type_hdr = b"Content-Type".to_vec();
    if !headers.contains_key(&content_type_hdr) {
        headers.insert(content_type_hdr,
                       writer.get_content_type().into_bytes());
    }

    let content_length_hdr = b"Content-Length".to_vec();
    if !headers.contains_key(&content_length_hdr) {
        match writer.get_content_length() {
            Some(i) => {
                headers.insert(
                    content_length_hdr,
                    i.to_string().into_bytes());
            },
            None => {}
        }
    }
}


fn handle_http<'req, R: Reader + Send + Sized, W: Writer + Send + Sized>
    (handler: &HTTPRequestHandler<'req, R, W>,
     mut reader: BufferedReader<R>,
     mut writer: BufferedWriter<W>)
     -> IoResult<()>
{
    let bad_req = proc
        (x: IoResult<Option<()>>)
         -> (IoResult<Option<()>>, HTTPMethod, Box<Vec<u8>>, Box<HTTPHeaders>)
    {
        (x, NoMethod, box b"".to_vec(), box HashMap::new())
    };

    let start_time = now().to_timespec();

    let (req_ok, request_method, request_path, request_headers) =
        match parse_http_request(&mut reader) {
            Ok(Some((m, p, h))) => (Ok(Some(())), m, p, h),
            Ok(None) => bad_req(Ok(None)),
            Err(e) => bad_req(Err(e)),
        };

    let request_duration = now().to_timespec() - start_time;

    let handler_result = match req_ok {
        Ok(Some(())) => handler.handle(
            request_method, &*request_path, &*request_headers, reader),
        Ok(None) => Ok(None),
        Err(e) => Err(e),
    };

    let (response_code, mut response_headers, response_writer) =
        match handler_result {
            Ok(Some((c, h, w))) => (c, h, w),
            _ => {
                let writer: Box<HTTPResponseWriter<W>> =
                    box BytesResponseWriter{bytes: vec![]};
                (HTTP400, box HashMap::new(), writer)
            },
        };

    update_response_headers(&*response_writer, &mut *response_headers);
    match start_http_response(&mut writer, response_code, &*response_headers) {
        Ok(_) => {
            let response_headers_duration = now().to_timespec() - start_time;
            let result = response_writer.write_data(writer);
            let response_end_duration = now().to_timespec() - start_time;

            info!("{} \"{}\" - {} (req: {:0.4f}s, resp: {:0.4f}s, end: {:0.4f}s)",
                  request_method,
                  match String::from_utf8(*request_path) {
                      Ok(s) => s,
                      Err(s) => format!("{}", s),
                  },
                  response_code as int,
                  request_duration.num_milliseconds() as f64 / 1000.0,
                  response_headers_duration.num_milliseconds() as f64 / 1000.0,
                  response_end_duration.num_milliseconds() as f64 / 1000.0
                  );
            result
        },
        Err(e) => Err(e)
    }
}


#[inline(always)]
fn parse_http_request<R: Reader + Send + Sized>
    (reader: &mut BufferedReader<R>)
     -> IoResult<Option<(
         HTTPMethod,
         Box<Vec<u8>>,  // path
         Box<HTTPHeaders>,  // request headers
         )>>
{
    let request_method = match try!(reader.read_until(b' ')).as_slice() {
        b"GET " => GET,
        b"POST " => POST,
        b"HEAD " => HEAD,
        _ => return Ok(None),
    };

    let request_path = {
        let mut s = box try!(reader.read_until(b' '));
        s.pop();
        s
    };

    match try!(reader.read_until(b'\n')).as_slice() {
        b"HTTP/1.0\r\n" | b"HTTP/1.1\r\n" => {},
        _ => return Ok(None),
    }

    let request_headers = {
        let mut headers = box HashMap::new();
        loop {
            let line = try!(reader.read_until('\n' as u8));

            if (line[line.len() - 2]) != '\r' as u8 { return Ok(None) }

            if line.len() == 2 { break }

            let colon_pos = match line.iter().position(|b| -> bool { *b == ':' as u8 }) {
                Some(0) | None => return Ok(None),
                Some(i) => i,
            };

            let value_start_pos = colon_pos + (
                match line.iter().skip(colon_pos).position(|b| -> bool { *b == ' ' as u8 }) {
                    Some(i) => i,
                    None => return Ok(None),
                });

            let value = line.slice(value_start_pos, line.len() - 2).to_vec();
            let mut key = line;
            key.truncate(colon_pos - 1);
            headers.insert(key, value);
        }
        headers
    };

    Ok(Some((request_method,
             request_path,
             request_headers,
             )))
}


pub struct BytesResponseWriter {
    bytes: Vec<u8>
}


impl <'a, W: Writer + Send + Sized>BytesResponseWriter {
    pub fn new(bytes: Vec<u8>) -> Box<HTTPResponseWriter<W> + 'a> {
        box BytesResponseWriter{bytes: bytes}
    }
}


impl <W: Writer + Send + Sized>HTTPResponseWriter<W>
    for BytesResponseWriter
{
    fn get_content_length(&self) -> Option<u64> {
        Some(self.bytes.len() as u64)
    }

    fn get_content_type(&self) -> String {
        "text/html; charset=utf-8".to_string()
    }

    fn write_data(&self, mut stream: BufferedWriter<W>) -> IoResult<()> {
        stream.write(self.bytes.as_slice())
    }
}


fn start_http_response<W: Writer>
    (writer: &mut BufferedWriter<W>,
     response_code: HTTPResponseCode,
     headers: &HTTPHeaders)
     -> IoResult<()>
{
    try!(writer.write_str("HTTP/1.0 "));
    try!(writer.write_str(match response_code {
        HTTP200 => "200 Ok",
        HTTP400 => "400 Bad Request",
        HTTP404 => "404 Not Found",
        _ => "417 I Am A Teapot",
    }));
    try!(writer.write_str("\r\n"));

    for (key, value) in headers.iter() {
        try!(writer.write(key.as_slice()));
        try!(writer.write_str(": "));
        try!(writer.write(value.as_slice()));
        try!(writer.write_str("\r\n"));
    }

    try!(writer.write_str("\r\n"));
    try!(writer.flush());
    Ok(())
}


pub fn multi_thread_http_serve
    <'req, T: HTTPRequestHandler<'req, TcpStream, TcpStream> + Send + Sync + Sized>
    (host: &str, port: u16, handler: Arc<T>)
     -> IoResult<()>
{
    let listener = TcpListener::bind(host, port);
    let mut acceptor = listener.listen();

    for stream in acceptor.incoming() {
        let stream = try!(stream);
        let new_handler = handler.clone();

        spawn(proc() {
            let reader = BufferedReader::new(stream.clone());
            let writer = BufferedWriter::new(stream);
            let result: IoResult<()> = handle_http(&*new_handler, reader, writer);
            result.unwrap()
        });
    }
    Ok(())
}


pub fn green_http_serve
    <'req, T: HTTPRequestHandler<'req, TcpStream, TcpStream> + Send + Sync + Sized>
    (host: &str, port: u16, handler: Arc<T>)
     -> IoResult<()>
{
    let mut pool = SchedPool::new(PoolConfig::new());

    let listener = TcpListener::bind(host, port);
    let mut acceptor = listener.listen();

    for stream in acceptor.incoming() {
        let stream = try!(stream);
        let new_handler = handler.clone();

        TaskBuilder::new().green(&mut pool).spawn(proc() {
            let reader = BufferedReader::new(stream.clone());
            let writer = BufferedWriter::new(stream);
            let result: IoResult<()> = handle_http(&*new_handler, reader, writer);
            result.unwrap()
        });
    }
    Ok(())
}
