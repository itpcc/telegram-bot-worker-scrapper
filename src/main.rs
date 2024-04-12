use std::str::FromStr;

use async_tungstenite::{
    tokio::connect_async,
    tungstenite::{handshake::client, http::Request, Message},
};
use deka_supremecourt_rs::{
    error,
    model::{MessagePayload, TGResponse},
    service::deka,
    util,
};
use futures::{prelude::*, SinkExt};

use snafu::{OptionExt, ResultExt};
use tokio::{
    signal::unix::{signal, SignalKind},
    sync::{broadcast, mpsc},
    task::LocalSet,
};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> util::Result<()> {
    const WS_ADDR: &str = "chatbotapi.itpcc.net";
    const TOKEN: &str = "DekaAtPauseman";

    // Setup tracing
    let sub = tracing_subscriber::fmt()
        .compact()
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_target(false)
        .with_max_level(LevelFilter::from_str("debug").context(error::LevelFilterSnafu)?)
        .finish();
    tracing::subscriber::set_global_default(sub)
        .ok()
        .context(error::GlobalDefautSnafu)?;
    tracing::debug!("init");

    let (sig_tx, _todo_sig_rx) = broadcast::channel(32);
    let (ws_tx, ws_rx) = mpsc::channel::<MessagePayload>(1000);
    let (tg_tx, mut tg_rx) = mpsc::channel::<TGResponse>(1000);
    let mut sigterm = signal(SignalKind::terminate()).context(error::SignalSnafu)?;
    let mut sigint = signal(SignalKind::interrupt()).context(error::SignalSnafu)?;

    let local_worker = LocalSet::new();
    let ws_req = Request::builder()
        .uri(format!("wss://{}/ws", WS_ADDR))
        .method("GET")
        .header("Host", WS_ADDR)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", client::generate_key())
        .header("Authorization", format!("Bearer {}", TOKEN))
        .body(())
        .context(error::HTTPSnafu)?;

    let (ws_strm, _) = connect_async(client::Request::from(ws_req))
        .await
        .context(error::WSSnafu)?;
    let (mut ws_write, mut ws_read) = ws_strm.split();

    tracing::info!("Main | Starting service thread");
    let dk_thd = tokio::spawn(deka::deka_thread(sig_tx.subscribe(), ws_rx, tg_tx.clone()));

    tracing::info!("Main | Starting main loop");
    local_worker
        .run_until(async {
            loop {
                tokio::select! {
                    Some(_) = sigterm.recv() => {
                        tracing::warn!("SIGTERM Shuting down");
                        let _ = sig_tx.send(());
                        break;
                    },
                    Some(_) = sigint.recv() => {
                        tracing::warn!("SIGINT Shuting down");
                        let _ = sig_tx.send(());
                        break;
                    },
                    Some(Ok(msg)) = ws_read.next() => {
                        if let Some(txt) = match msg {
                            Message::Text(msg_txt) => Some(msg_txt),
                            Message::Binary(msg_bin) => String::from_utf8(msg_bin).ok(),
                            _ => {
                                tracing::debug!("WS Non-text message: {:?}", msg);
                                None
                            }
                        } {
                            tracing::debug!("ws_read text message: {:?}", txt);

                            match serde_json::from_str::<MessagePayload>(&txt) {
                                Ok(pld) => {
                                    tracing::debug!("ws_read Payload: {:?}", pld);
                                    while let Err(e) = ws_tx.send(pld.clone()).await.context(error::SendSnafu) {
                                        tracing::warn!("ws_read Send error: {:?}", e);
                                    }
                                },
                                Err(e) => {
                                    tracing::warn!("ws_read Deserialize error: {:?}", e);
                                }
                            }
                        }
                    },
                    Some(tg_msg) = tg_rx.recv() => {
                        if  let Ok(ws_msg) = serde_json::to_string(&tg_msg) {
                            if let Err(e) = ws_write.send(Message::Text(ws_msg)).await {
                                tracing::warn!("ws_write Tx error: {:?}", e);
                            }
                        }
                    }
                }
            }
        })
        .await;
    ws_write.close().await.context(error::WSSnafu)?;
    tracing::info!("Main | Stopping Services.");
    local_worker.await;
    dk_thd.abort();
    tracing::info!("Main | Services ended.");
    Ok(())
}
