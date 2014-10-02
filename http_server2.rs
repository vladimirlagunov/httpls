#![crate_name="http_server2"]
#![crate_type="lib"]
#![feature(macro_rules, phase)]
#![allow(experimental)]

#[phase(plugin,link)] extern crate log;

use std::io::{TcpListener, TcpStream, BufferedReader, BufferedWriter, IoResult, Reader, Buffer, Acceptor, Listener};
use std::collections::HashMap;


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


pub trait HTTPRequestHandler<'hndl, 'req, R: Reader, W: Writer> {
    fn handle(
        &'hndl self,
        method: HTTPMethod,
        path: Box<Vec<u8>>,
        headers: Box<HTTPHeaders>,
        mut stream: BufferedReader<R>)
        -> IoResult<Option<(HTTPResponseCode,
                            Box<HTTPHeaders>,
                            Box<HTTPResponseWriter<W> + 'req>)>>;
}


pub trait HTTPResponseWriter<W: Writer> {
    fn write_data(&self, mut stream: BufferedWriter<W>) -> IoResult<()>;
}


pub struct HTTPHandler<'hndl, 'req, R: Reader, W: Writer> {
    pub handler: Box<HTTPRequestHandler<'hndl, 'req, R, W> + 'hndl>,
    pub host: String
}


impl <'hndl, 'resp, R: Reader, W: Writer>HTTPHandler<'hndl, 'resp, R, W> {
    fn handle(&'hndl self,
              reader: BufferedReader<R>,
              mut writer: BufferedWriter<W>)
              -> IoResult<()>
    {
        let (response_code, response_headers, response_writer) =
            match handle_http_request(&*self.handler, reader) {
                Err(_) | Ok(None) => bad_response(),
                    // (HTTP400, box HashMap::new(), box BytesResponseWriter{bytes: vec![]}),
                Ok(Some(x)) => x
            };
        match start_http_response(&mut writer, response_code, &*response_headers) {
            Ok(_) => {
                response_writer.write_data(writer)
            },
            Err(e) => Err(e)
        }
    }
}

fn bad_response<'resp, W: Writer>() -> (HTTPResponseCode, Box<HTTPHeaders>, Box<HTTPResponseWriter<W> + 'resp>) {
    (HTTP400, box HashMap::new(), box BytesResponseWriter{bytes: vec![]})
}


fn handle_http_request<'hndl, 'req, R: Reader, W: Writer>
    (handler: &'hndl HTTPRequestHandler<'hndl, 'req, R, W>,
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


impl <'a, W: Writer>BytesResponseWriter {
    pub fn new(bytes: Vec<u8>) -> Box<HTTPResponseWriter<W> + 'a> {
        box BytesResponseWriter{bytes: bytes}
    }
}


impl <W: Writer>HTTPResponseWriter<W> for BytesResponseWriter {
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
    Ok(())
}



pub struct SingleThreadHTTPServer {
    pub host: String,
    pub port: u16,
}


impl <'server, 'hndl, 'req>SingleThreadHTTPServer {
    pub fn serve(&'server self,
                 handler: &'hndl HTTPHandler<'hndl, 'req, TcpStream, TcpStream>)
                 -> IoResult<()> {
        let listener = TcpListener::bind(self.host.as_slice(), self.port);

        let mut acceptor = listener.listen();

        for stream in acceptor.incoming() {
            let stream = try!(stream);

            let reader = BufferedReader::new(stream.clone());
            let writer = BufferedWriter::new(stream);

            try!(handler.handle(reader, writer));
        }
        Ok(())
    }
}
