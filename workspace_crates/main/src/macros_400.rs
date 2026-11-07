#[macro_export]
macro_rules! ok_or_400 {
    ($expr:expr $(, $msg:expr)?) => {
        match $expr {
            Ok(v) => v,
            Err(e) => {
                $(
                    println!("Err: {:#?}, Msg: {:#?}", e, $msg);
                )?
                return server::http_stuff::http_response::bad_request_400();
            }
        }
    };
}
#[macro_export]
macro_rules! ok_or_err_400 {
    ($expr:expr $(, $msg:expr)?) => {
        match $expr {
            Ok(v) => v,
            Err(e) => {
                $(
                    println!("Err: {}, Msg: {}", e, $msg);
                )?
                return Err(server::http_stuff::http_response::bad_request_400());
            }
        }
    };
}
#[macro_export]
macro_rules! some_or_400 {
    ($expr:expr $(, $msg:expr)?) => {
        match $expr {
            Some(v) => v,
            None => {
                $(
                    println!("{}", $msg);
                )?
                return server::http_stuff::http_response::bad_request_400();
            }
        }
    };
}
#[macro_export]
macro_rules! some_or_err_400 {
    ($expr:expr $(, $msg:expr)?) => {
        match $expr {
            Some(v) => v,
            None => {
                $(
                    println!("{}", $msg);
                )?
                return Err(server::http_stuff::http_response::bad_request_400());
            }
        }
    };
}
