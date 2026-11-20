//! Bounded text-only subset of the Robin protobuf and gRPC contracts.
use super::{AppError, AppModel};
use axum::http::StatusCode;
use bytes::{Buf, BytesMut};
use std::{future::poll_fn, pin::Pin};

const ORIGIN: &str = "https://robinfrontend-pa.googleapis.com";
const PATH: &str = "/assistant.mobile.robin.RobinService/ProcessQuery";
const MAX_FRAME: usize = 1024 * 1024;
const MAX_WIRE: usize = 64 * 1024 * 1024;
const MAX_FRAMES: usize = 4096;

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7f {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}
fn number(out: &mut Vec<u8>, tag: u64, value: u64) {
    put_varint(out, tag << 3);
    put_varint(out, value);
}
fn blob(out: &mut Vec<u8>, tag: u64, data: &[u8]) {
    put_varint(out, tag << 3 | 2);
    put_varint(out, data.len() as u64);
    out.extend_from_slice(data);
}
fn nested(out: &mut Vec<u8>, tag: u64, write: impl FnOnce(&mut Vec<u8>)) {
    let mut child = Vec::new();
    write(&mut child);
    blob(out, tag, &child);
}
fn frame(out: &mut Vec<u8>, write: impl FnOnce(&mut Vec<u8>)) {
    let mut msg = Vec::new();
    write(&mut msg);
    out.push(0);
    out.extend_from_slice(&(msg.len() as u32).to_be_bytes());
    out.extend(msg);
}
fn initial(prompt: &str, mode: u64, thinking: u64) -> Vec<u8> {
    let mut metadata = Vec::new();
    nested(&mut metadata, 1, |f| number(f, 1, 1));
    nested(&mut metadata, 2, |f| blob(f, 1, b"en"));
    nested(&mut metadata, 3, |f| blob(f, 1, b"1.135.1.1291"));
    number(&mut metadata, 4, 21);
    number(&mut metadata, 5, 1);
    blob(&mut metadata, 7, b"en");
    blob(
        &mut metadata,
        23,
        uuid::Uuid::new_v4().to_string().as_bytes(),
    );
    let mut wire = Vec::new();
    frame(&mut wire, |f| {
        number(f, 1, 1);
        nested(f, 1000, |f| {
            blob(f, 1, &metadata);
            nested(f, 1001, |f| {
                nested(f, 1, |f| {
                    nested(f, 1, |f| blob(f, 1, prompt.as_bytes()));
                    number(f, 5, 1);
                    number(f, 15, 1); // no built-in tools
                    number(f, 31, 1); // temporary chat
                    number(f, 47, mode);
                    number(f, 48, thinking);
                    number(f, 55, 0); // no overage spending
                    number(f, 56, 0); // no automatic model substitution
                })
            });
            blob(f, 1004, b"");
        });
    });
    frame(&mut wire, |f| {
        number(f, 1, 1);
        nested(f, 1007, |f| blob(f, 1, prompt.as_bytes()));
    });
    frame(&mut wire, |f| {
        number(f, 1, 1);
        nested(f, 1007, |f| blob(f, 2, b""));
    });
    wire
}

fn invalid() -> AppError {
    AppError::upstream(
        "gemini_app_invalid_protocol",
        "Gemini App returned an unsupported or malformed response.",
    )
}
fn take<'a>(bytes: &mut &'a [u8], size: usize) -> Result<&'a [u8], AppError> {
    if size > bytes.len() {
        return Err(invalid());
    }
    let (data, rest) = bytes.split_at(size);
    *bytes = rest;
    Ok(data)
}
fn varint(bytes: &mut &[u8]) -> Result<u64, AppError> {
    let mut out = 0;
    for i in 0..10 {
        let b = take(bytes, 1)?[0];
        if i == 9 && b > 1 {
            return Err(invalid());
        }
        out |= u64::from(b & 0x7f) << (i * 7);
        if b < 128 {
            return Ok(out);
        }
    }
    Err(invalid())
}
#[derive(Clone, Copy)]
enum Field<'a> {
    Number(u64),
    Bytes(&'a [u8]),
    Ignored,
}
fn fields(mut bytes: &[u8]) -> Result<Vec<(u64, Field<'_>)>, AppError> {
    let mut fields = Vec::new();
    while !bytes.is_empty() {
        let key = varint(&mut bytes)?;
        if key >> 3 == 0 || key >> 3 >= 1 << 29 || fields.len() >= 4096 {
            return Err(invalid());
        }
        let value = match key & 7 {
            0 => Field::Number(varint(&mut bytes)?),
            1 | 5 => {
                take(&mut bytes, if key & 7 == 1 { 8 } else { 4 })?;
                Field::Ignored
            }
            2 => {
                let size = usize::try_from(varint(&mut bytes)?).map_err(|_| invalid())?;
                Field::Bytes(take(&mut bytes, size)?)
            }
            _ => return Err(invalid()),
        };
        fields.push((key >> 3, value));
    }
    Ok(fields)
}
fn one<'a>(fields: &[(u64, Field<'a>)], tag: u64) -> Result<Option<Field<'a>>, AppError> {
    let mut matches = fields.iter().filter(|(id, _)| *id == tag);
    let out = matches.next().map(|(_, value)| *value);
    if matches.next().is_some() {
        return Err(invalid());
    }
    Ok(out)
}
fn child<'a>(fields: &[(u64, Field<'a>)], tag: u64) -> Result<Option<&'a [u8]>, AppError> {
    match one(fields, tag)? {
        Some(Field::Bytes(bytes)) => Ok(Some(bytes)),
        None => Ok(None),
        _ => Err(invalid()),
    }
}
fn scalar(fields: &[(u64, Field<'_>)], tag: u64) -> Result<Option<u64>, AppError> {
    match one(fields, tag)? {
        Some(Field::Number(value)) => Ok(Some(value)),
        None => Ok(None),
        _ => Err(invalid()),
    }
}
fn text<'a>(fields: &[(u64, Field<'a>)], tag: u64) -> Result<Option<&'a str>, AppError> {
    child(fields, tag)?
        .map(|raw| std::str::from_utf8(raw).map_err(|_| invalid()))
        .transpose()
}

#[derive(Default)]
struct TurnState {
    id: Option<String>,
    full: String,
    completed: bool,
    verified: bool,
}
impl TurnState {
    fn accept(&mut self, data: &[u8], model: &AppModel) -> Result<Option<String>, AppError> {
        let outer = fields(data)?;
        let Some(wrapper) = child(&outer, 1003)? else {
            return Ok(None);
        };
        let wrapper = fields(wrapper)?;
        let Some(msg) = child(&wrapper, 1)? else {
            return Ok(None);
        };
        let msg = fields(msg)?;
        let done = scalar(&msg, 9)?.unwrap_or(0);
        let fallback = scalar(&msg, 11)?.unwrap_or(0);
        let downgrade = scalar(&msg, 31)?.unwrap_or(0);
        let mode = scalar(&msg, 41)?.unwrap_or(0);
        let attribution = text(&msg, 30)?.unwrap_or("");
        if done > 1 || fallback > 1 || one(&msg, 7)?.is_some() {
            return Err(invalid());
        }
        if fallback == 1 || downgrade != 0 || (mode != 0 && mode != model.mode) {
            return Err(if matches!(downgrade, 2 | 3 | 6 | 8) {
                AppError::quota()
            } else {
                AppError::model_mismatch()
            });
        }
        let mut snapshot = None;
        if let Some(data) = child(&msg, 1)? {
            let data = fields(data)?;
            if let Some(id) = text(&data, 1)?.filter(|id| !id.is_empty()) {
                if id.len() > 4096 || self.id.as_deref().is_some_and(|prior| prior != id) {
                    return Err(invalid());
                }
                if self.id.is_none() {
                    self.id = Some(id.to_owned());
                }
            }
            if let Some(display) = child(&data, 2)? {
                snapshot = text(&fields(display)?, 1)?;
            }
        }
        if mode != 0 || done == 1 || snapshot.is_some_and(|s| !s.is_empty()) {
            let correct_attribution = model.attribution.is_none_or(|wanted| {
                attribution
                    .split(|ch: char| !ch.is_ascii_alphanumeric())
                    .any(|word| word.eq_ignore_ascii_case(wanted))
            });
            if !correct_attribution || (mode == 0 && !self.verified) {
                return Err(AppError::model_mismatch());
            }
            self.verified = true;
        }
        self.completed |= done == 1;
        if let Some(snapshot) = snapshot.filter(|text| !text.is_empty()) {
            let delta = snapshot.strip_prefix(&self.full).ok_or_else(|| {
                AppError::upstream(
                    "gemini_app_snapshot_rewritten",
                    "Gemini App rewrote previously received text.",
                )
            })?;
            if !delta.is_empty() {
                let delta = delta.to_owned();
                self.full = snapshot.to_owned();
                return Ok(Some(delta));
            }
        }
        Ok(None)
    }
}

pub(super) struct Turn {
    body: reqwest::Body,
    buffer: BytesMut,
    state: TurnState,
    grpc: Option<u32>,
    frames: usize,
    bytes: usize,
    ended: bool,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

fn grpc_status(headers: &axum::http::HeaderMap) -> Result<Option<u32>, AppError> {
    let mut values = headers.get_all("grpc-status").iter();
    let status = values.next();
    if values.next().is_some() {
        return Err(invalid());
    }
    status
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|s| s.parse().ok())
                .ok_or_else(invalid)
        })
        .transpose()
}
fn grpc_error(status: u32) -> AppError {
    match status {
        4 => AppError::timeout(),
        8 => AppError::quota(),
        7 | 16 => AppError::forbidden(),
        9 => AppError::upstream("gemini_app_precondition_required", "Gemini App rejected an account prerequisite; check access and terms in the official app."),
        _ => AppError::upstream("gemini_app_upstream_failed", "Gemini App rejected the generation."),
    }
}

pub(super) async fn open(
    client: &reqwest::Client,
    token: &str,
    permit: tokio::sync::OwnedSemaphorePermit,
    model: &AppModel,
    prompt: &str,
    thinking: u64,
) -> Result<Turn, AppError> {
    let response = client
        .post(format!("{ORIGIN}{PATH}"))
        .version(axum::http::Version::HTTP_2)
        .bearer_auth(token)
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .header("grpc-timeout", "180S")
        .body(initial(prompt, model.mode, thinking))
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                AppError::timeout()
            } else {
                AppError::upstream(
                    "gemini_app_connection_failed",
                    "Could not reach Gemini App.",
                )
            }
        })?;
    parse_response(response, permit)
}

fn parse_response(
    response: reqwest::Response,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Result<Turn, AppError> {
    match response.status() {
        StatusCode::OK => {}
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => return Err(AppError::forbidden()),
        StatusCode::TOO_MANY_REQUESTS => return Err(AppError::quota()),
        _ => {
            return Err(AppError::upstream(
                "gemini_app_http_rejected",
                "Gemini App rejected the HTTP request.",
            ))
        }
    }
    if response.version() != axum::http::Version::HTTP_2
        || !response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|v| v == "application/grpc" || v.starts_with("application/grpc+"))
    {
        return Err(invalid());
    }
    let grpc = grpc_status(response.headers())?;
    if let Some(code) = grpc.filter(|code| *code != 0) {
        return Err(grpc_error(code));
    }
    Ok(Turn {
        body: response.into(),
        buffer: BytesMut::new(),
        state: TurnState::default(),
        grpc,
        frames: 0,
        bytes: 0,
        ended: false,
        _permit: permit,
    })
}

impl Turn {
    pub(super) async fn next(&mut self, model: &AppModel) -> Result<Option<String>, AppError> {
        if self.ended {
            return Ok(None);
        }
        loop {
            if self.buffer.len() >= 5 {
                let size = u32::from_be_bytes(self.buffer[1..5].try_into().unwrap()) as usize;
                if self.buffer[0] != 0 || size > MAX_FRAME {
                    return Err(invalid());
                }
                if self.buffer.len() >= size + 5 {
                    self.frames += 1;
                    if self.frames > MAX_FRAMES {
                        return Err(invalid());
                    }
                    self.buffer.advance(5);
                    let frame = self.buffer.split_to(size);
                    if let Some(delta) = self.state.accept(&frame, model)? {
                        return Ok(Some(delta));
                    }
                    continue;
                }
            }
            match poll_fn(|cx| hyper::body::Body::poll_frame(Pin::new(&mut self.body), cx)).await {
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) => {
                        self.bytes += data.len();
                        if self.bytes > MAX_WIRE
                            || self.buffer.len().saturating_add(data.len()) > 4 * MAX_FRAME
                        {
                            return Err(invalid());
                        }
                        self.buffer.extend_from_slice(&data);
                    }
                    Err(frame) => {
                        if let Ok(trailers) = frame.into_trailers() {
                            if let Some(status) = grpc_status(&trailers)? {
                                if self.grpc.is_some_and(|prior| status != prior) {
                                    return Err(invalid());
                                }
                                self.grpc = Some(status);
                            }
                        }
                    }
                },
                Some(Err(error)) => {
                    return Err(if error.is_timeout() {
                        AppError::timeout()
                    } else {
                        AppError::upstream(
                            "gemini_app_stream_interrupted",
                            "Gemini App stream was interrupted.",
                        )
                    })
                }
                None => {
                    self.ended = true;
                    if let Some(status) = self.grpc.filter(|code| *code != 0) {
                        return Err(grpc_error(status));
                    }
                    if !self.buffer.is_empty()
                        || self.grpc != Some(0)
                        || !self.state.completed
                        || !self.state.verified
                        || self.state.full.trim().is_empty()
                    {
                        return Err(AppError::upstream(
                            "gemini_app_incomplete",
                            "Gemini App did not produce a verified complete response.",
                        ));
                    }
                    return Ok(None);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::upstream::gemini_app::MODELS;

    fn fixture(mode: u64, attribution: &[u8], snapshot: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        nested(&mut data, 1003, |f| {
            nested(f, 1, |f| {
                number(f, 41, mode);
                blob(f, 30, attribution);
                nested(f, 1, |f| nested(f, 2, |f| blob(f, 1, snapshot)));
            })
        });
        data
    }

    #[test]
    fn validates_model_and_monotonic_snapshots() {
        let mut turn = TurnState::default();
        assert_eq!(
            turn.accept(&fixture(3, b"Gemini Neon", b"first"), &MODELS[0])
                .unwrap()
                .as_deref(),
            Some("first")
        );
        assert_eq!(
            turn.accept(&fixture(3, b"Gemini Neon", b"first second"), &MODELS[0])
                .unwrap()
                .as_deref(),
            Some(" second")
        );
        assert!(turn
            .accept(&fixture(3, b"Gemini Neon", b"rewrite"), &MODELS[0])
            .is_err());
        assert!(turn
            .accept(&fixture(1, b"Gemini Hydrogen", b"first second"), &MODELS[0])
            .is_err());
        assert!(turn
            .accept(&fixture(3, b"Someone else", b"first second"), &MODELS[0])
            .is_err());
        assert!(varint(&mut &[255; 10][..]).is_err());
    }

    #[tokio::test]
    async fn requires_grpc_success_and_model_completion_over_http2() {
        use axum::{body::Bytes, http::HeaderMap, response::Response};
        use hyper::{
            body::{Body, Frame, Incoming},
            server::conn::http2,
            service::service_fn,
        };
        use hyper_util::rt::{TokioExecutor, TokioIo};
        use std::{
            convert::Infallible,
            task::{Context, Poll},
        };

        struct Fixture(std::collections::VecDeque<Frame<Bytes>>);
        impl Body for Fixture {
            type Data = Bytes;
            type Error = Infallible;
            fn poll_frame(
                mut self: Pin<&mut Self>,
                _: &mut Context<'_>,
            ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
                Poll::Ready(self.0.pop_front().map(Ok))
            }
        }
        for (done, trailer, complete) in [
            (true, Some("0"), true),
            (false, Some("0"), false),
            (true, None, false),
            (true, Some("8"), false),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (io, _) = listener.accept().await.unwrap();
                let handler = service_fn(move |_: axum::http::Request<Incoming>| async move {
                    let mut wire = Vec::new();
                    frame(&mut wire, |f| {
                        let mut message = Vec::new();
                        nested(&mut message, 1, |f| {
                            number(f, 41, 3);
                            blob(f, 30, b"Gemini Neon");
                            number(f, 9, u64::from(done));
                            nested(f, 1, |f| nested(f, 2, |f| blob(f, 1, b"fixture")));
                        });
                        blob(f, 1003, &message);
                    });
                    let rest = wire.split_off(3);
                    let mut frames = std::collections::VecDeque::from([
                        Frame::data(Bytes::from(wire)),
                        Frame::data(Bytes::from(rest)),
                    ]);
                    if let Some(status) = trailer {
                        let mut trailers = HeaderMap::new();
                        trailers.insert("grpc-status", status.parse().unwrap());
                        frames.push_back(Frame::trailers(trailers));
                    }
                    Ok::<_, Infallible>(
                        Response::builder()
                            .header("content-type", "application/grpc")
                            .body(Fixture(frames))
                            .unwrap(),
                    )
                });
                let _ = http2::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(io), handler)
                    .await;
            });
            let client = reqwest::Client::builder()
                .no_proxy()
                .http2_prior_knowledge()
                .timeout(std::time::Duration::from_secs(3))
                .build()
                .unwrap();
            let response = client.get(url).send().await.unwrap();
            let permit = std::sync::Arc::new(tokio::sync::Semaphore::new(1))
                .try_acquire_owned()
                .unwrap();
            let mut turn = parse_response(response, permit).unwrap();
            assert_eq!(
                turn.next(&MODELS[0]).await.unwrap().as_deref(),
                Some("fixture")
            );
            let result = turn.next(&MODELS[0]).await;
            if complete {
                assert!(result.unwrap().is_none());
            } else if trailer == Some("8") {
                assert_eq!(result.unwrap_err().status, StatusCode::TOO_MANY_REQUESTS);
            } else {
                assert_eq!(result.unwrap_err().code, "gemini_app_incomplete");
            }
            server.abort();
        }
    }

    #[test]
    fn initial_contains_three_separate_grpc_messages() {
        let wire = initial("hello", 3, 2);
        let mut rest = wire.as_slice();
        for tag in [1000, 1007, 1007] {
            let header = take(&mut rest, 5).unwrap();
            assert_eq!(header[0], 0);
            let size = u32::from_be_bytes(header[1..].try_into().unwrap());
            let body = fields(take(&mut rest, size as usize).unwrap()).unwrap();
            assert!(child(&body, tag).unwrap().is_some());
        }
        assert!(rest.is_empty());
    }
}
