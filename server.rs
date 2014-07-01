#![feature(macro_rules)]
#![allow(experimental)]
extern crate green;
extern crate rustuv;

use std::io::{TcpStream, BufferedReader, IoResult, Reader, Buffer, IoError};

#[start]
fn start(argc: int, argv: **u8) -> int {
    green::start(argc, argv, rustuv::event_loop, main)
}


fn main() {
    let listen_addr = String::from_str("127.0.0.1");
    let listen_port = 8080;
}


enum HTTPMethod {
    GET, POST, HEAD
}


trait HTTPMethodConvert {
    fn to_httpmethod(&self) -> Option<HTTPMethod>;
}

impl HTTPMethodConvert for String {
    fn to_httpmethod(&self) -> Option<HTTPMethod> {
        self.as_slice().to_httpmethod()
    }
}


impl <'a>HTTPMethodConvert for &'a str {
    fn to_httpmethod(&self) -> Option<HTTPMethod> {
        match *self {
            "GET" => Some(GET),
            "POST" => Some(POST),
            "HEAD" => Some(HEAD),
            _ => None
        }
    }
}


struct HTTPRequest {
    method: HTTPMethod,
    path: String,
    headers: Vec<HTTPHeader>
}

impl HTTPRequest {
    fn new(method: HTTPMethod, path: String, headers: Vec<HTTPHeader>) -> HTTPRequest {
        HTTPRequest{method: method, path: path, headers: headers}
    }
}

struct HTTPHeader {
    key: String,
    value: String
}


enum HTTPResponseCode {
    HTTP_200,
    HTTP_301, HTTP_302,
    HTTP_400, HTTP_403, HTTP_404,
    HTTP_500
}


static CARRIAGE_RETURN: u8 = 10;
static NEW_LINE: u8 = 13;


fn http_handler(mut stream: TcpStream) {
    let mut reader = BufferedReader::with_capacity(4000, stream);
    let _ = _http_get_request_and_headers(reader);
}

enum HttpParseError {
    IoError(IoError), ParseError
}


fn _http_get_request_and_headers<R: Buffer>
    (ref mut reader: R)
     -> Result<HTTPRequest, HttpParseError>
{
    let first_line = try!(_http_read_line(reader));
    let mut first_line_iter = first_line.as_slice().split(' ');

    let method = match first_line_iter.next() {
        Some(method) => match method.to_httpmethod() {
            Some(method) => method,
            None => return Err(ParseError)
        },
        None => return Err(ParseError)
    };

    let path = match first_line_iter.next() {
        Some(path) => path,
        None => return Err(ParseError)
    };

    let mut headers = Vec::<HTTPHeader>::new();
    loop {
        let line = try!(_http_read_line(reader));
        if (line.as_slice().chars().count() == 0) { break; };

        let mut header_iter = line.as_slice().splitn(':', 1);
        match (header_iter.next(), header_iter.next()) {
            (Some(key), Some(value)) => {
                headers.push(HTTPHeader{
                    key: String::from_str(key), value: String::from_str(value)
                })
            },
            _ => return Err(ParseError)
        };
    }

    Ok(HTTPRequest::new(method, String::from_str(""), headers))
}

fn _http_read_line<R: Buffer>(mut reader: &mut R) -> Result<String, HttpParseError> {
    let result = match reader.read_until(CARRIAGE_RETURN) {
        Ok(line_bytes) => match String::from_utf8(line_bytes) {
            Ok(line) => line,
            _ => return Err(ParseError)
        },
        Err(e) => return Err(IoError(e))
    };
    match reader.read_char() {
        Ok(' ') => Ok(result),
        Ok(_) => Err(ParseError),
        Err(e) => Err(IoError(e))
    }
}


// Может лучше proc() ?
trait HTTPServer {
    fn handle(method: HTTPMethod, headers: Vec<HTTPHeader>, stream: TcpStream);
}
