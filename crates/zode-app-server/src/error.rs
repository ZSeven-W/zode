use zode_app_server_protocol::rpc::ErrorObject;

pub type ServerResult<T> = Result<T, ErrorObject>;

pub fn error(code: i64, message: impl Into<String>) -> ErrorObject {
    ErrorObject {
        code,
        message: message.into(),
        data: None,
    }
}
