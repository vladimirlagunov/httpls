#![feature(macro_rules)]
#![allow(experimental)]
extern crate green;
extern crate rustuv;

use std::io::{TcpListener, TcpStream, BufferedReader, BufferedWriter, IoResult, Reader, Buffer, IoError, Acceptor, Listener};

#[start]
fn start(argc: int, argv: **u8) -> int {
    green::start(argc, argv, rustuv::event_loop, main)
}


fn main() {
    let listen_addr = "127.0.0.1";
    let listen_port = 8080;
    run_server(listen_addr, listen_port);
}


fn run_server(listen_addr: &str, listen_port: u16) -> IoResult<()> {
    let listener = try!(TcpListener::bind(listen_addr, listen_port));
    let mut acceptor = try!(listener.listen());

    loop {
        match acceptor.accept() {
            Ok(mut stream) => http_handler(stream),
            Err(e) => println!("{}", e)
        }
    }

    Ok(())
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

impl HTTPHeader {
    fn new(key: String, value: String) -> HTTPHeader {
        HTTPHeader{key: key, value: value}
    }
}

enum HTTPResponseCode {
    HTTP_200,
    HTTP_301, HTTP_302,
    HTTP_400, HTTP_403, HTTP_404,
    HTTP_500
}


static CARRIAGE_RETURN: u8 = 10;
static NEW_LINE: u8 = 13;


fn make_http_400() -> HTTPResponse {
    HTTPResponse::new(
        HTTP_400,
        vec![HTTPHeader::new(String::from_str("Content-Type"),
                             String::from_str("text-html; charset=utf-8"))],
        box "<h1>Bad request</h1>".bytes())
}

fn make_http_500() -> HTTPResponse {
    HTTPResponse::new(
        HTTP_500,
        vec![HTTPHeader::new(String::from_str("Content-Type"),
                             String::from_str("text-html; charset=utf-8"))],
        box "<h1>Server error</h1>".bytes())
}


fn http_handler(mut stream: TcpStream) {
    let mut reader = BufferedReader::with_capacity(4000, stream.clone());
    let response = match _http_get_request_and_headers(reader) {
        Ok((request, reader)) => {
            match handler(request, reader) {
                Ok(response) => Some(response),
                _ => Some(make_http_500())
            }
        },
        Err(ParseError) => {
            Some(make_http_400())
        },
        Err(IoError(_)) => None
    };
    match response {
        Some(response) => {
            _http_send_response(response, stream);
        },
        None => {}
    };
    ()
}


enum HttpParseError {
    IoError(IoError), ParseError
}


fn _http_get_request_and_headers<R: Buffer>
    (mut reader: R)
     -> Result<(HTTPRequest, R), HttpParseError>
{
    let first_line = try!(_http_read_line(&mut reader));
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

    let mut headers = Vec::<HTTPHeader>::with_capacity(16);
    loop {
        let line = try!(_http_read_line(&mut reader));
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

    Ok((HTTPRequest::new(method, String::from_str(""), headers),
        reader))
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


struct HTTPResponse {
    code: HTTPResponseCode,
    headers: Vec<HTTPHeader>,
    content: Box<Iterator<u8>>
}

impl HTTPResponse {
    fn new(code: HTTPResponseCode, headers: Vec<HTTPHeader>, content: Box<Iterator<u8>>)
           -> HTTPResponse {
        HTTPResponse{code: code, headers: headers, content: content}
    }
}


fn _http_send_response(mut response: HTTPResponse, stream: TcpStream) -> IoResult<()> {
    let mut writer = BufferedWriter::with_capacity(1500, stream.clone());

    try!(writer.write_str("HTTP "));
    try!(writer.write_str(match response.code {
        HTTP_200 => "200 OK",
        HTTP_301 => "301 Moved",
        HTTP_302 => "302 Moved Permanently",
        HTTP_400 => "400 Bad Request",
        HTTP_403 => "403 Not Authorized",
        HTTP_404 => "404 Not Found",
        HTTP_500 => "500 Server Error"
    }));
    try!(writer.write_u8(CARRIAGE_RETURN));
    try!(writer.write_u8(NEW_LINE));
    let mut headers = response.headers;
    loop {
        match headers.pop() {
            Some(header) => {
                try!(writer.write(header.key.into_bytes().as_slice()));
                try!(writer.write_str(": "));
                try!(writer.write(header.value.into_bytes().as_slice()));
                try!(writer.write_u8(CARRIAGE_RETURN));
                try!(writer.write_u8(NEW_LINE));
            },
            None => break
        }
    }

    try!(writer.write_u8(CARRIAGE_RETURN));
    try!(writer.write_u8(NEW_LINE));

    for byte in response.content {
        try!(writer.write_u8(byte))
    }

    Ok(())
}


fn handler<R: Reader>(request: HTTPRequest, ref mut reader: R) -> Result<HTTPResponse, ()> {
    Ok(HTTPResponse::new(
        HTTP_200,
        vec![HTTPHeader::new(String::from_str("Content-Type"),
                             String::from_str("text-html; charset=utf-8"))],
        box "<h1>Hello world!</h1>".bytes()))
}
