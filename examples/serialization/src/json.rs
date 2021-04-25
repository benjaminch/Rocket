use std::borrow::Cow;

use rocket::State;
use rocket::tokio::sync::Mutex;
use rocket_contrib::json::{Json, JsonValue, json};

use serde::{Serialize, Deserialize};

// The type to represent the ID of a message.
type Id = usize;

// We're going to store all of the messages here. No need for a DB.
type MessageList = Mutex<Vec<String>>;
type Messages<'r> = State<'r, MessageList>;

#[derive(Serialize, Deserialize)]
struct Message<'r> {
    id: Option<Id>,
    message: Cow<'r, str>
}

#[post("/", format = "json", data = "<message>")]
async fn new(message: Json<Message<'_>>, list: Messages<'_>) -> JsonValue {
    let mut list = list.lock().await;
    let id = list.len();
    list.push(message.message.to_string());
    json!({ "status": "ok", "id": id })
}

#[put("/<id>", format = "json", data = "<message>")]
async fn update(id: Id, message: Json<Message<'_>>, list: Messages<'_>) -> Option<JsonValue> {
    match list.lock().await.get_mut(id) {
        Some(existing) => {
            *existing = message.message.to_string();
            Some(json!({ "status": "ok" }))
        }
        None => None
    }
}

#[get("/<id>", format = "json")]
async fn get<'r>(id: Id, list: Messages<'r>) -> Option<Json<Message<'r>>> {
    let list = list.lock().await;

    Some(Json(Message {
        id: Some(id),
        message: list.get(id)?.to_string().into(),
    }))
}

#[catch(404)]
fn not_found() -> JsonValue {
    json!({
        "status": "error",
        "reason": "Resource was not found."
    })
}

pub fn stage() -> rocket::fairing::AdHoc {
    rocket::fairing::AdHoc::on_ignite("JSON", |rocket| async {
        rocket.mount("/json", routes![new, update, get])
            .register("/json", catchers![not_found])
            .manage(MessageList::new(vec![]))
    })
}
