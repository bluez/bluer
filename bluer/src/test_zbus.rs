use zbus::Connection;
use zbus::MessageStream;

async fn test() {
    let connection = Connection::session().await.unwrap();
    let stream = MessageStream::from(&connection);
}
