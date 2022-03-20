#[macro_use]
extern crate dotenv_codegen;

use anyhow::Result;
use std::sync::Arc;
use tokio::time::Duration;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use webrtc::rtp_transceiver::{rtp_codec::RTPCodecType, rtp_receiver::RTCRtpReceiver};
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::{TrackLocal, TrackLocalWriter};
use webrtc::track::track_remote::TrackRemote;
use webrtc::Error;

mod signal;

#[tokio::main]
async fn main() -> Result<()> {
    let port = dotenv!("PORT");
    println!("{}", port);
    let mut sdp_channel_rx = signal::http_sdp_server(port.to_string().parse::<u16>()?).await;

    println!("wait for the offer from http_sdp_server\n");
    let line = sdp_channel_rx.recv().await.unwrap();
    let desc_data = signal::decode(line.as_str())?;
    let offer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;

    let mut media_engine = MediaEngine::default();

    media_engine.register_default_codecs()?;

    let mut registry = Registry::new();

    registry = register_default_interceptors(registry, &mut media_engine)?;

    let api = APIBuilder::new()
        .with_media_engine(media_engine)
        .with_interceptor_registry(registry)
        .build();

    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_owned()],
            ..Default::default()
        }],
        ..Default::default()
    };

    let peer_connection = Arc::new(api.new_peer_connection(config).await?);

    peer_connection
        .add_transceiver_from_kind(RTPCodecType::Video, &[])
        .await?;

    let (local_track_channel_tx, mut local_track_channel_rx) =
        tokio::sync::mpsc::channel::<Arc<TrackLocalStaticRTP>>(1);

    let local_track_chan_tx = Arc::new(local_track_channel_tx);
    let peer_connect = Arc::downgrade(&peer_connection);

    peer_connection
        .on_track(Box::new(
            move |track: Option<Arc<TrackRemote>>, _receiver: Option<Arc<RTCRtpReceiver>>| {
                if let Some(track) = track {
                    let media_ssrc = track.ssrc();
                    let peer_connect_clone = peer_connect.clone();

                    tokio::spawn(async move {
                        let mut result = Result::<usize>::Ok(0);
                        while result.is_ok() {
                            let timeout = tokio::time::sleep(Duration::from_secs(3));
                            tokio::pin!(timeout);

                            tokio::select! {
                                _ = timeout.as_mut() =>{
                                    if let Some(pc) = peer_connect_clone.upgrade(){
                                        result = pc.write_rtcp(&[Box::new(PictureLossIndication{
                                            sender_ssrc: 0,
                                            media_ssrc,
                                        })]).await.map_err(Into::into);
                                    }else{
                                        break;
                                    }
                                }
                            };
                        }
                    });

                    let local_track_chan_tx2 = Arc::clone(&local_track_chan_tx);
                    tokio::spawn(async move {
                        let local_track = Arc::new(TrackLocalStaticRTP::new(
                            track.codec().await.capability,
                            "video".to_owned(),
                            "webrtc-rs".to_owned(),
                        ));
                        let _ = local_track_chan_tx2.send(Arc::clone(&local_track)).await;

                        while let Ok((rtp, _)) = track.read_rtp().await {
                            if let Err(err) = local_track.write_rtp(&rtp).await {
                                if Error::ErrClosedPipe != err {
                                    print!("output track write_rtp got error: {:?} and break", err);
                                    break;
                                } else {
                                    print!("output track write_rtp got error: {:?}", err);
                                }
                            }
                        }
                    });
                }

                Box::pin(async {})
            },
        ))
        .await;

    peer_connection
        .on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            println!("Peer Connection State has changed: {}", s);
            Box::pin(async {})
        }))
        .await;

    peer_connection.set_remote_description(offer).await?;

    let answer = peer_connection.create_answer(None).await?;

    let mut gather_complete = peer_connection.gathering_complete_promise().await;

    peer_connection.set_local_description(answer).await?;

    gather_complete.recv().await;

    if let Some(local_desc) = peer_connection.local_description().await {
        let json_str = serde_json::to_string(&local_desc)?;
        let b64 = signal::encode(&json_str);
        println!("{}", b64);
    } else {
        println!("generate local_description failed!");
    }

    if let Some(local_track) = local_track_channel_rx.recv().await {
        loop {
            println!("\nCurl an base64 SDP to start sendonly peer connection");

            let line = sdp_channel_rx.recv().await.unwrap();
            let desc_data = signal::decode(line.as_str())?;
            let recv_only_offer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;

            let mut media_engine = MediaEngine::default();

            media_engine.register_default_codecs()?;

            let mut registry = Registry::new();

            registry = register_default_interceptors(registry, &mut media_engine)?;

            let api = APIBuilder::new()
                .with_media_engine(media_engine)
                .with_interceptor_registry(registry)
                .build();

            let config = RTCConfiguration {
                ice_servers: vec![RTCIceServer {
                    urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                    ..Default::default()
                }],
                ..Default::default()
            };

            let peer_connection = Arc::new(api.new_peer_connection(config).await?);

            let rtp_sender = peer_connection
                .add_track(Arc::clone(&local_track) as Arc<dyn TrackLocal + Send + Sync>)
                .await?;

            tokio::spawn(async move {
                let mut rtcp_buf = vec![0u8; 1500];
                while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
                Result::<()>::Ok(())
            });

            peer_connection
                .on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
                    println!("Peer Connection State has changed: {}", state);
                    Box::pin(async {})
                }))
                .await;

            peer_connection
                .set_remote_description(recv_only_offer)
                .await?;

            let answer = peer_connection.create_answer(None).await?;

            let mut gather_complete = peer_connection.gathering_complete_promise().await;

            peer_connection.set_local_description(answer).await?;

            let _ = gather_complete.recv().await;

            if let Some(local_desc) = peer_connection.local_description().await {
                let json_str = serde_json::to_string(&local_desc)?;
                let b64 = signal::encode(&json_str);
                println!("{}", b64);
            } else {
                println!("generate local_description failed!");
            }
        }
    }

    Ok(())
}
