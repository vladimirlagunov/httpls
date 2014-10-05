#![crate_name="http_server2"]
#![crate_type="lib"]
#![feature(macro_rules, phase)]
#![allow(experimental)]

#[phase(plugin,link)] extern crate log;

use std::io::{TcpListener, TcpStream, BufferedReader, BufferedWriter, IoResult, Reader, Buffer, Acceptor, Listener};
use std::collections::HashMap;
use std::task::{TaskBuilder};
use std::sync::Arc;

#[deriving(Show)]
pub enum HTTPMethod {
    GET, POST, HEAD
}


#[deriving(Show)]
pub enum HTTPResponseCode {
    HTTP200,
    HTTP301, HTTP302,
    HTTP400, HTTP401, HTTP403, HTTP404,
    HTTP500
}


pub type HTTPHeaders = HashMap<Vec<u8>, Vec<u8>>;


pub trait HTTPRequestHandler
    <'req, R: Reader + Send + Sized, W: Writer + Send + Sized>
    : Send + Sized
{
    fn handle(
        &self,
        method: HTTPMethod,
        path: Box<Vec<u8>>,
        headers: Box<HTTPHeaders>,
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
     reader: BufferedReader<R>,
     mut writer: BufferedWriter<W>)
     -> IoResult<()>
{
    let (response_code, mut response_headers, response_writer) =
        match handle_http_request(handler, reader) {
            Err(_) | Ok(None) => bad_response(),
            Ok(Some(x)) => x
        };
    update_response_headers(&*response_writer, &mut *response_headers);
    match start_http_response(&mut writer, response_code, &*response_headers) {
        Ok(_) => {
            response_writer.write_data(writer)
        },
        Err(e) => Err(e)
    }
}

fn bad_response<'resp, W: Writer + Send + Sized>
    ()
     -> (HTTPResponseCode, Box<HTTPHeaders>,
         Box<HTTPResponseWriter<W> + 'resp>)
{
    (HTTP400, box HashMap::new(), box BytesResponseWriter{bytes: vec![]})
}


fn handle_http_request
    <'req, R: Reader + Send + Sized, W: Writer + Send + Sized>
    (handler: &HTTPRequestHandler<'req, R, W>,
     mut reader: BufferedReader<R>)
     -> IoResult<Option<(HTTPResponseCode,
                         Box<HTTPHeaders>,
                         Box<HTTPResponseWriter<W> + 'req>)>>
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

    handler.handle(request_method, request_path, request_headers, reader)
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
            handle_http(&*new_handler, reader, writer);
        });
    }
    Ok(())
}
