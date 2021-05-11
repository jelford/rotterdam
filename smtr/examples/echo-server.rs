use json::object;
use json::JsonValue;

use smtr::{
    server::{serve, Response},
    Method,
};

fn main() {
    pretty_env_logger::init_timed();
    let requests = serve("127.0.0.1:8508").unwrap();
    println!("Listening on port 8508");
    for (mut req, resp) in requests {
        let mut result = JsonValue::new_object();
        result["path"] = JsonValue::String(req.path().to_string());
        result["method"] = JsonValue::String(req.method().as_str().to_string());

        result["args"] = JsonValue::Array(
            req.query_pairs()
                .iter()
                .map(|(name, value)| object!("name": name.to_string(), "value": value.to_string()))
                .collect::<Vec<_>>(),
        );

        result["headers"] = JsonValue::Array(
            req.headers()
                .iter()
                .map(|(name, value)| {
                    let name = String::from_utf8_lossy(&name.as_header_string()).to_string();
                    object!("name": name, "value": String::from_utf8_lossy(value).to_string())
                })
                .collect::<Vec<_>>(),
        );

        match req.method() {
            Method::POST | Method::PUT => {
                result["body"] = req
                    .body().unwrap()
                    .map(|b| JsonValue::String(base64::encode_config(b, base64::URL_SAFE)))
                    .unwrap_or(JsonValue::Null);
            }
            _ => { /* no body in get/option/delete requests */}
        };

        resp.send_response(
            Response::builder(200)
                .body_from_string(&result.to_string())
                .build(),
        )
        .unwrap();
    }
}
