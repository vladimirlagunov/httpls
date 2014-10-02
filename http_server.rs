#![crate_name="httpls"]
#![crate_type="lib"]
#![feature(macro_rules, phase)]
#![allow(experimental)]

#[phase(plugin,link)] extern crate log;

use std::io::{TcpListener, TcpStream, BufferedReader, BufferedWriter, IoResult, Reader, Buffer, IoError, Acceptor, Listener};
use std::rc::Rc;


pub enum HTTPMethod {
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


impl std::from_str::FromStr for HTTPMethod {
    fn from_str(s: &str) -> Option<HTTPMethod> {
        match s {
            "GET" => Some(GET),
            "POST" => Some(POST),
            "HEAD" => Some(HEAD),
            _ => None
        }
    }
}


pub struct HTTPRequest {
    pub method: HTTPMethod,
    pub path: String,
    pub headers: Vec<HTTPHeader>
}


pub struct HTTPHeader {
    pub key: String,
    pub value: String
}

impl HTTPHeader {
    fn new(key: String, value: String) -> HTTPHeader {
        HTTPHeader{key: key, value: value}
    }
}

pub enum HTTPResponseCode {
    HTTP200,
    HTTP301, HTTP302,
    HTTP400, HTTP401, HTTP403, HTTP404,
    HTTP500
}


enum HttpParseError {
    IOError(IoError), ParseError
}

pub struct HTTPResponse {
    pub code: HTTPResponseCode,
    pub headers: Vec<HTTPHeader>,
    pub content_length: Option<u64>,
}


pub trait HTTPHandler<T> {
    pub fn handle(request: &HTTPRequest)
}

pub type HttpHandlerFn = fn (Rc<HTTPRequest>, BufferedReader<TcpStream>)
                             -> Result<(HTTPResponse, ResponseWriterFn), ()>;
pub type ResponseWriterFn = fn (BufferedWriter<TcpStream>) -> IoResult<()>;


impl std::fmt::Show for HTTPResponseCode {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::FormatError> {
        write!(f, "{}", match *self {
            HTTP200 => 200u16,
            HTTP301 => 301, HTTP302 => 302,
            HTTP400 => 400, HTTP401 => 401, HTTP403 => 403, HTTP404 => 404,
            HTTP500 => 500
        })
    }
}

pub fn http_handler(mut stream: TcpStream, handler: HttpHandlerFn) {
    let mut reader = BufferedReader::with_capacity(1500, stream.clone());
    let peer_name = stream.peer_name();

    let (request, response_pair): (
        Option<Rc<HTTPRequest>>, Option<(HTTPResponse, ResponseWriterFn)>
            ) = match _http_get_request_and_headers(&mut reader) {
        Ok(request) => {
            let request = Rc::new(request);
            let response_pair = match handler(request.clone(), reader) {
                Ok(response_pair) => response_pair,
                _ => error_page_response(HTTP500)
            };
            (Some(request), Some(response_pair))
        },
        Err(ParseError) => {
            (None, Some(error_page_response(HTTP400)))
        },
        Err(IOError(_)) => (None, None)
    };

    match response_pair {
        Some((response, response_fn)) => {
            let code = response.code;
            let error = match _http_send_response(response, response_fn, stream) {
                Ok(()) => false,
                Err(_) => true
            };
            let peer_name = match peer_name {
                Ok(addr) => format!("{}", addr),
                _ => String::from_str("???")
            };
            match (request, error) {
                (Some(ref request), false) =>
                    info!("[{}] {} \"{}\" => {}",
                               peer_name, request.method, request.path, code),
                (Some(ref request), true) =>
                    info!("[{}] {} \"{}\" => {} (NOT SENT)",
                          peer_name, request.method, request.path, code),
                _ =>
                    info!("[{}] ??? => {}", peer_name, code)
            };
        },
        None => {}
    };
    ()
}

static CARRIAGE_RETURN: u8 = 13;
static NEW_LINE: u8 = 10;
static RN: [u8, ..2] = [CARRIAGE_RETURN, NEW_LINE];


pub fn error_page_response(response_code: HTTPResponseCode)
                       -> (HTTPResponse, ResponseWriterFn)
{
    let response_content = format!(
        "<h1>{} {}</h1>",
        response_code,
        match response_code {
            HTTP400 => " Bad Request",
            HTTP401 => " Method Not Allowed",
            HTTP403 => " Access Denied",
            HTTP404 => " Not Found",
            HTTP500 => " Server Error",
            _ => ""
        });
    let response_content_length = response_content.as_bytes().iter().count();

    let response = HTTPResponse {
        code: response_code,
        headers: vec![HTTPHeader{
            key: String::from_str("Content-Type"),
            value: String::from_str("text-html; charset=utf-8")
        }],
        content_length: Some(response_content_length as u64),
    };
    let response_fn: ResponseWriterFn
        = proc(mut buf: BufferedWriter<TcpStream>) -> IoResult<()> {
            buf.write(response_content.as_bytes())
        };
    (response, response_fn)
}


fn _http_get_request_and_headers
    (reader: &mut BufferedReader<TcpStream>) -> Result<HTTPRequest, HttpParseError>
{
    let first_line = try!(_http_read_line(reader));
    let mut first_line_iter = first_line.as_slice().split(' ');

    let method = match first_line_iter.next() {
        Some(method) => match from_str(method) {
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
        let line = try!(_http_read_line(reader));
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

    Ok(HTTPRequest{method: method, path: path, headers: headers})
}


fn _http_read_line(reader: &mut BufferedReader<TcpStream>)
                   -> Result<String, HttpParseError>
{
    let result = match reader.read_until(CARRIAGE_RETURN) {
        Ok(line_bytes) => {
            let line_string = match String::from_utf8(line_bytes) {
                Ok(line) => line,
                _ => return Err(ParseError)
            };
            let line_bytes = line_string.as_slice().trim_right_chars(CARRIAGE_RETURN as char);
            String::from_str(line_bytes)
        },
        Err(e) => return Err(IOError(e))
    };
    match reader.read_u8() {
        Ok(NEW_LINE) => Ok(result),
        Ok(_) => Err(ParseError),
        Err(e) => Err(IOError(e))
    }
}


fn _http_send_response(response: HTTPResponse,
                       response_fn: ResponseWriterFn,
                       stream: TcpStream)
                       -> IoResult<()> {
    let mut writer = BufferedWriter::with_capacity(1500, stream.clone());

    try!(writer.write_str("HTTP "));
    try!(writer.write_str(match response.code {
        HTTP200 => "200 OK",
        HTTP301 => "301 Moved",
        HTTP302 => "302 Moved Permanently",
        HTTP400 => "400 Bad Request",
        HTTP401 => "401 Method Not Allowed",
        HTTP403 => "403 Not Authorized",
        HTTP404 => "404 Not Found",
        HTTP500 => "500 Server Error"
    }));
    try!(writer.write(RN));
    let mut headers = response.headers;
    loop {
        match headers.pop() {
            Some(header) => {
                try!(write!(writer, "{}: {}", header.key, header.value));
                try!(writer.write(RN));
            },
            None => break
        }
    }

    try!(writer.write_u8(CARRIAGE_RETURN));
    try!(writer.write_u8(NEW_LINE));

    try!(writer.flush());

    try!(response_fn(BufferedWriter::with_capacity(1500, stream)));

    Ok(())
}
