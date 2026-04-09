use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum LogicRequest {
    SetProperty { path: String, property: String, value: String },
    GetProperty { path: String, property: String },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum LogicResponse {
    PropertyValue { value: String },
    Error { message: String },
}
