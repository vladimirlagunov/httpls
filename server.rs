#![feature(macro_rules)]
#![feature(phase)]
#![allow(experimental)]
extern crate green;
extern crate rustuv;
#[phase(plugin, link)] extern crate log;

use std::io::{TcpListener, TcpStream, BufferedReader, BufferedWriter, IoResult, Reader, Buffer, IoError, Acceptor, Listener};

#[start]
fn start(argc: int, argv: *const *const u8) -> int {
    green::start(argc, argv, rustuv::event_loop, main)
}


fn main() {
    let listen_addr = "127.0.0.1";
    let listen_port = 8080;
    match run_server(listen_addr, listen_port) {
        Ok(_) => {},
        Err(e) => fail!(e)
    };
}


fn run_server(listen_addr: &str, listen_port: u16) -> IoResult<()> {
    let listener = try!(TcpListener::bind(listen_addr, listen_port));
    let mut acceptor = try!(listener.listen());

    loop {
        match acceptor.accept() {
            Ok(stream) => http_handler(stream),
            Err(e) => println!("{}", e)
        }
    }

    Ok(())
}


enum HTTPMethod {
    GET, POST, HEAD
}

impl std::fmt::Show for HTTPMethod {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> Result<(), std::fmt::FormatError> {
        match *self {
            GET => fmt.write("GET".as_bytes()),
            POST => fmt.write("POST".as_bytes()),
            HEAD => fmt.write("HEAD".as_bytes())
        }
    }
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
    HTTP200,
    HTTP301, HTTP302,
    HTTP400, HTTP403, HTTP404,
    HTTP500
}


enum HttpParseError {
    IoError(IoError), ParseError
}

struct HTTPResponse {
    code: HTTPResponseCode,
    headers: Vec<HTTPHeader>,
    content_writer: proc(mut writer: BufferedWriter<TcpStream>)
}


impl std::fmt::Show for HTTPResponseCode {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::FormatError> {
        write!(f, "{}", match *self {
            HTTP200 => 200u16,
            HTTP301 => 301, HTTP302 => 302,
            HTTP400 => 400, HTTP403 => 403, HTTP404 => 404,
            HTTP500 => 500
        })
    }
}



static CARRIAGE_RETURN: u8 = 13;
static NEW_LINE: u8 = 10;
static RN: [u8, ..2] = [CARRIAGE_RETURN, NEW_LINE];


fn error_page_response(response_code: HTTPResponseCode) -> HTTPResponse {
    HTTPResponse {
        code: response_code,
        headers: vec![HTTPHeader::new(String::from_str("Content-Type"),
                                      String::from_str("text-html; charset=utf-8"))],
        content_writer: proc(mut buf) {
            buf.write_str("<h1>");
            buf.write(format!("{}", response_code).as_bytes());
            buf.write_str(match response_code {
                HTTP400 => " Bad Request",
                HTTP403 => " Access Denied",
                HTTP404 => " Not Found",
                HTTP500 => " Server Error",
                _ => ""
            });
            buf.write_str("</h1>");
        }
    }
}


fn http_handler(mut stream: TcpStream) {
    let reader = BufferedReader::with_capacity(1500, stream.clone());
    let peer_name = stream.peer_name();
    let (request, response) = match _http_get_request_and_headers(reader) {
        Ok((request, reader)) => {
            let response = match handler(&request, reader) {
                Ok(response) => Some(response),
                _ => Some(error_page_response(HTTP500))
            };
            (Some(request), response)
        },
        Err(ParseError) => {
            (None, Some(error_page_response(HTTP400)))
        },
        Err(IoError(_)) => (None, None)
    };
    match response {
        Some(response) => {
            let code = response.code;
            _http_send_response(response, stream);
            let peer_name = match peer_name {
                Ok(addr) => format!("{}", addr),
                _ => String::from_str("???")
            };
            match request {
                Some(request) =>
                    info!("[{}] {} \"{}\" => {}",
                          peer_name, request.method, request.path, code),
                None =>
                    info!("[{}] ??? => {}", peer_name, code)
            };
        },
        None => {}
    };
    ()
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
        Some(path) => String::from_str(path),
        None => return Err(ParseError)
    };

    let mut headers = Vec::<HTTPHeader>::with_capacity(16);
    loop {
        let line = try!(_http_read_line(&mut reader));
        if line.as_slice().chars().count() == 0 { break; };

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

    Ok((HTTPRequest::new(method, path, headers), reader))
}


fn _http_read_line<R: Buffer>(reader: &mut R) -> Result<String, HttpParseError> {
    let result = match reader.read_until(CARRIAGE_RETURN) {
        Ok(line_bytes) => {
            let line_string = match String::from_utf8(line_bytes) {
                Ok(line) => line,
                _ => return Err(ParseError)
            };
            let line_bytes = line_string.as_slice().trim_right_chars(CARRIAGE_RETURN as char);
            String::from_str(line_bytes)
        },
        Err(e) => return Err(IoError(e))
    };
    match reader.read_u8() {
        Ok(NEW_LINE) => Ok(result),
        Ok(_) => Err(ParseError),
        Err(e) => Err(IoError(e))
    }
}


fn _http_send_response(mut response: HTTPResponse, stream: TcpStream) -> IoResult<()> {
    let mut writer = BufferedWriter::with_capacity(1500, stream.clone());

    try!(writer.write_str("HTTP "));
    try!(writer.write_str(match response.code {
        HTTP200 => "200 OK",
        HTTP301 => "301 Moved",
        HTTP302 => "302 Moved Permanently",
        HTTP400 => "400 Bad Request",
        HTTP403 => "403 Not Authorized",
        HTTP404 => "404 Not Found",
        HTTP500 => "500 Server Error"
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

    writer.flush();

    let content_writer: proc(BufferedWriter<TcpStream>) = response.content_writer;
    content_writer(BufferedWriter::with_capacity(1500, stream));

    Ok(())
}


fn handler<R: Reader>(request: &HTTPRequest, ref reader: R) -> Result<HTTPResponse, ()> {
    let response = match (request.method, &request.path) {
        (GET, path) if path == &String::from_str("/") =>
            HTTPResponse { 
                code: HTTP200,
                headers: vec![HTTPHeader::new(String::from_str("Content-Type"),
                                              String::from_str("text-html; charset=utf-8"))],
                content_writer: proc(mut buf) {
                    buf.write_str("<h1>Hello world!</h1>");
                }
            },
        (GET, _) => error_page_response(HTTP404),
        _ => error_page_response(HTTP400)
    };
    Ok(response)
}
