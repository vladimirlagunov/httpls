#![feature(phase, macro_rules)]
#![allow(experimental)]

extern crate http_server2;

use std::collections::HashMap;

use std::io::{
    BufferedReader,
    BufferedWriter,
    IoResult,
};

use std::io::timer::sleep;

use std::rand::random;

use std::sync::Arc;

use std::time::duration::Duration;

use http_server2::{
    multi_thread_http_serve,
    green_http_serve,
    BytesResponseWriter,
    HTTPRequestHandler,
    HTTPMethod,
    HTTPHeaders,
    HTTPResponseCode, HTTP200,
    HTTPResponseWriter,
};


fn main() {
    // multi_thread_http_serve(
    green_http_serve(
        "127.0.0.1", 8080, Arc::new(
        HelloWorldHTTPHandler
        // StaticHandler{bytes: b"Hello world!\r\n".to_vec()}
            )).unwrap();
}


struct HelloWorldHTTPHandler;

impl <'req, R: Reader + Send + Sized, W: Writer + Send + Sized>
    HTTPRequestHandler<'req, R, W>
    for HelloWorldHTTPHandler
{
    fn handle(
        &self,
        method: HTTPMethod,
        path: &Vec<u8>,
        headers: &HTTPHeaders,
        stream: BufferedReader<R>)
        -> IoResult<Option<(HTTPResponseCode,
                            Box<HTTPHeaders>,
                            Box<HTTPResponseWriter<W> + 'req>)>>
    {
        drop(stream);

        let writer: Box<HTTPResponseWriter<W>> =
            box StreamingHelloWorldResponseWriter{
                count: random::<u32>() % 10 + 1,
                sleep: 1000, content_length: true,
                s: "Hello world!",
            };

        Ok(Some((HTTP200, box HashMap::new(), writer)))
    }
}


struct StreamingHelloWorldResponseWriter<W: Writer + Send + Sized> {
    count: u32,
    sleep: u32,
    content_length: bool,
    s: &'static str,
}


impl <W: Writer + Send + Sized> HTTPResponseWriter<W>
    for StreamingHelloWorldResponseWriter<W>
{
    fn get_content_length(&self) -> Option<u64> {
        if self.content_length {
            let mut len = 0u64;
            let mut exp = 1u64;
            let mut val = 1u64;
            while val * 10 < self.count as u64 {
                val *= 10;
                len += val * (exp + 5);
                exp += 1;
            }
            len += (self.count as u64 - val + 1) * (exp + 5);

            Some(len + self.s.len() as u64 + 2)
        } else {
            None
        }
    }

    fn get_content_type(&self) -> String {
        "text/html; charset=utf-8".to_string()
    }

    fn write_data(&self, mut stream: BufferedWriter<W>) -> IoResult<()> {
        let mut count = self.count;
        while count > 0 {
            try!(stream.write_str(format!("{}...\r\n", count).as_slice()));
            try!(stream.flush());
            count -= 1;
            sleep(Duration::milliseconds(self.sleep as i64));
        }
        try!(stream.write_str(self.s));
        try!(stream.write_str("\r\n"));
        stream.flush()
    }

}


pub struct StaticHandler {
    bytes: Vec<u8>
}


impl <'req, R: Reader + Send + Sized, W: Writer + Send + Sized>
    HTTPRequestHandler<'req, R, W>
    for StaticHandler
{
    fn handle(
        &self,
        method: HTTPMethod,
        path: &Vec<u8>,
        headers: &HTTPHeaders,
        stream: BufferedReader<R>)
        -> IoResult<Option<(HTTPResponseCode,
                            Box<HTTPHeaders>,
                            Box<HTTPResponseWriter<W> + 'req>)>>
    {
        drop(stream);

        let writer: Box<HTTPResponseWriter<W>> =
            BytesResponseWriter::<W>::new(self.bytes.clone());

        Ok(Some((HTTP200, box HashMap::new(), writer)))
    }
}
