//! External application API: the JSON protocol (envelope, versions,
//! validation, methods), independent of the transport.
//! spec: 70-advanced/external-api.md §2–§4

use std::collections::HashSet;

use serde_json::{json, Map, Value};

/// (version string, numeric code), oldest first (§2).
pub const VERSIONS: [(&str, u32); 6] = [
    ("0.9.0", 900),
    ("0.9.1", 901),
    ("0.9.2", 902),
    ("0.9.3", 903),
    ("1.0.0", 10000),
    ("1.0.1", 10001),
];
pub const LATEST: u32 = 10001;
pub const DEFAULT_PORT: u16 = 22033;

pub fn version_code(s: &str) -> Option<u32> {
    VERSIONS.iter().find(|v| v.0 == s).map(|v| v.1)
}

pub fn version_name(c: u32) -> &'static str {
    VERSIONS.iter().find(|v| v.1 == c).map_or("1.0.1", |v| v.0)
}

/// Error types (§2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiError {
    InvalidJson,
    UnsupportedVersion,
    MethodNotFound,
    InvalidType,
    InvalidData,
    InvalidParameter,
    InvalidModel,
    InvalidDocument,
    InvalidView,
    PluginNotRegistered,
}

impl ApiError {
    pub fn name(self) -> &'static str {
        match self {
            ApiError::InvalidJson => "InvalidJson",
            ApiError::UnsupportedVersion => "UnsupportedVersion",
            ApiError::MethodNotFound => "MethodNotFound",
            ApiError::InvalidType => "InvalidType",
            ApiError::InvalidData => "InvalidData",
            ApiError::InvalidParameter => "InvalidParameter",
            ApiError::InvalidModel => "InvalidModel",
            ApiError::InvalidDocument => "InvalidDocument",
            ApiError::InvalidView => "InvalidView",
            ApiError::PluginNotRegistered => "PluginNotRegistered",
        }
    }
}

/// Document kinds of GetDocuments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    Modeling,
    Animation,
    Physics,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DocInfo {
    pub uid: String,
    pub kind: DocKind,
    pub path: String,
    /// (ModelUID, model file path for animation views).
    pub views: Vec<(String, Option<String>)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamInfo {
    pub id: String,
    pub name: String,
    pub group_uid: String,
    pub default: f32,
    pub max: f32,
    pub min: f32,
    pub repeat: bool,
    /// 0 normal, 1 blend shape.
    pub kind: u8,
    /// Sorted key values used on the parameter (1.0.1).
    pub keys: Vec<f32>,
}

/// What the editor exposes to the API.
pub trait Host {
    fn documents(&self) -> Vec<DocInfo>;
    fn current_document(&self) -> Option<String>;
    /// The active model; `animation` = whether animation documents count
    /// (0.9.1+: the selected model track).
    fn current_model(&self, animation: bool) -> Option<String>;
    fn edit_mode(&self) -> &'static str;
    /// Parameters of a model (by ModelUID or DocumentUID, else current).
    /// `animation` = whether animation scenes may answer (0.9.1+).
    fn parameters(
        &self,
        model: Option<&str>,
        doc: Option<&str>,
        animation: bool,
    ) -> Result<Vec<ParamInfo>, ApiError>;
    fn groups(
        &self,
        model: Option<&str>,
        doc: Option<&str>,
        animation: bool,
    ) -> Result<Vec<(String, String)>, ApiError>;
    fn values(&self, model: &str) -> Result<Vec<(String, f32)>, ApiError>;
    fn set_values(&mut self, model: &str, values: &[(String, f32)]) -> Result<(), ApiError>;
    fn clear_values(&mut self, model: &str) -> Result<(), ApiError>;
    fn log(&mut self, warning: bool, message: &str, display: bool);
    /// A plugin registers: returns (token, already approved).
    fn register(
        &mut self,
        name: &str,
        token: Option<&str>,
        path: Option<&str>,
        icon: Option<&str>,
    ) -> (String, bool);
    fn approved(&self, token: &str) -> bool;
}

/// One connection's state.
#[derive(Clone, Debug, Default)]
pub struct Session {
    /// Set by SetGlobalVersion; otherwise each request's own Version.
    pub version: Option<u32>,
    pub token: Option<String>,
    pub notify: HashSet<String>,
}

/// A message to send, after `delay_ms`.
#[derive(Clone, Debug, PartialEq)]
pub struct Outgoing {
    pub text: String,
    pub delay_ms: u64,
}

/// Method variants: the API versions that introduced a variant (§4.1).
fn variants(method: &str) -> Option<&'static [u32]> {
    Some(match method {
        "GetAPIVersion" | "SetGlobalVersion" | "ClearParameterValues" => &[901],
        "RegisterPlugin" | "GetIsApprove" | "GetCurrentEditMode" => &[900],
        "GetCurrentModelUID" | "GetDocuments" | "GetParameterGroups" | "GetParameterValues"
        | "SetParameterValues" => &[900, 901],
        "GetCurrentDocumentUID" | "GetDocument" | "SendCubismLog" => &[903],
        "GetParameters" => &[900, 901, 10001],
        "NotifyMocFileExported" => &[900, 903],
        "NotifyChangeEditMode"
        | "NotifyMotionFileExported"
        | "NotifyPhysicsFileExported"
        | "NotifyMotionSyncFileExported" => &[900],
        _ => return None,
    })
}

/// Methods allowed before approval.
fn open_method(m: &str) -> bool {
    matches!(
        m,
        "RegisterPlugin" | "GetIsApprove" | "GetAPIVersion" | "SetGlobalVersion"
    )
}

/// A response / event / error envelope, null keys omitted (§4.1).
pub fn envelope(
    version: &str,
    now_ms: u64,
    request: Option<&str>,
    kind: &str,
    method: Option<&str>,
    data: Value,
) -> String {
    let mut m = Map::new();
    m.insert("Version".into(), json!(version));
    m.insert("Timestamp".into(), json!(now_ms));
    if let Some(r) = request {
        m.insert("RequestId".into(), json!(r));
    }
    m.insert("Type".into(), json!(kind));
    if let Some(x) = method {
        m.insert("Method".into(), json!(x));
    }
    m.insert("Data".into(), data);
    Value::Object(m).to_string()
}

fn error(version: &str, now: u64, req: Option<&str>, method: Option<&str>, e: ApiError) -> String {
    envelope(
        version,
        now,
        req,
        "Error",
        method,
        json!({ "ErrorType": e.name() }),
    )
}

/// Optional key of the right type (absent or null is fine).
fn opt_str<'a>(d: &'a Value, k: &str) -> Result<Option<&'a str>, ApiError> {
    match d.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        _ => Err(ApiError::InvalidData),
    }
}

fn req_str<'a>(d: &'a Value, k: &str) -> Result<&'a str, ApiError> {
    opt_str(d, k)?.ok_or(ApiError::InvalidData)
}

fn param_json(p: &ParamInfo, with_keys: bool) -> Value {
    let mut v = json!({
        "Id": p.id, "Name": p.name, "GroupUID": p.group_uid, "Default": p.default,
        "Max": p.max, "Min": p.min, "Repeat": p.repeat, "Type": p.kind,
    });
    if with_keys {
        v["Keyform"] = Value::Array(p.keys.iter().map(|k| json!({ "Value": k })).collect());
    }
    v
}

fn doc_json(d: &DocInfo, with_uid: bool) -> Value {
    let mut m = Map::new();
    if with_uid {
        m.insert("DocumentUID".into(), json!(d.uid));
    }
    m.insert("DocumentFilePath".into(), json!(d.path));
    match d.kind {
        DocKind::Modeling => {
            m.insert(
                "Views".into(),
                Value::Array(d.views.iter().map(|v| json!({ "ModelUID": v.0 })).collect()),
            );
        }
        DocKind::Animation => {
            m.insert(
                "Views".into(),
                Value::Array(
                    d.views
                        .iter()
                        .map(|v| json!({ "ModelUID": v.0, "ModelFilePath": v.1.clone().unwrap_or_default() }))
                        .collect(),
                ),
            );
        }
        DocKind::Physics => {
            if let Some(v) = d.views.first() {
                m.insert("ModelUID".into(), json!(v.0));
            }
        }
    }
    Value::Object(m)
}

/// Handles one incoming text frame.
pub fn handle(text: &str, s: &mut Session, host: &mut dyn Host, now: u64) -> Vec<Outgoing> {
    let out = |t: String| {
        vec![Outgoing {
            text: t,
            delay_ms: 0,
        }]
    };
    let latest = version_name(LATEST);
    let Ok(msg) = serde_json::from_str::<Value>(text) else {
        return out(error(latest, now, None, None, ApiError::InvalidJson));
    };
    let (Some(method), Some(kind)) = (
        msg.get("Method").and_then(Value::as_str),
        msg.get("Type").and_then(Value::as_str),
    ) else {
        return out(error(
            latest,
            now,
            msg.get("RequestId").and_then(Value::as_str),
            None,
            ApiError::InvalidJson,
        ));
    };
    let req = msg.get("RequestId").and_then(Value::as_str);
    let Some(client) = msg
        .get("Version")
        .and_then(Value::as_str)
        .and_then(version_code)
    else {
        return out(error(
            latest,
            now,
            req,
            Some(method),
            ApiError::UnsupportedVersion,
        ));
    };
    if kind != "Request" {
        return out(error(latest, now, req, Some(method), ApiError::InvalidType));
    }
    let session = s.version.unwrap_or(client);
    let Some(vars) = variants(method) else {
        return out(error(
            version_name(session),
            now,
            req,
            Some(method),
            ApiError::MethodNotFound,
        ));
    };
    let Some(&variant) = vars.iter().rev().find(|&&v| v <= session) else {
        return out(error(
            version_name(session),
            now,
            req,
            Some(method),
            ApiError::UnsupportedVersion,
        ));
    };
    let vname = version_name(variant);
    let data = msg.get("Data").cloned().unwrap_or(json!({}));
    if !open_method(method) && !s.token.as_deref().is_some_and(|t| host.approved(t)) {
        return out(error(
            vname,
            now,
            req,
            Some(method),
            ApiError::PluginNotRegistered,
        ));
    }
    let result = run(method, variant, &data, s, host);
    match result {
        Ok((d, delay)) => vec![Outgoing {
            text: envelope(vname, now + delay, req, "Response", Some(method), d),
            delay_ms: delay,
        }],
        Err(e) => out(error(vname, now, req, Some(method), e)),
    }
}

fn run(
    method: &str,
    variant: u32,
    d: &Value,
    s: &mut Session,
    host: &mut dyn Host,
) -> Result<(Value, u64), ApiError> {
    let ok = |v: Value| Ok((v, 0));
    let anim = variant >= 901;
    match method {
        "GetAPIVersion" => ok(json!({ "Version": version_name(LATEST) })),
        "SetGlobalVersion" => {
            let v = req_str(d, "Version")?;
            let c = version_code(v).ok_or(ApiError::UnsupportedVersion)?;
            s.version = Some(c);
            ok(json!({}))
        }
        "RegisterPlugin" => {
            let name = req_str(d, "Name")?;
            let token = opt_str(d, "Token")?;
            let icon = opt_str(d, "Icon")?;
            let path = opt_str(d, "Path")?;
            let (t, approved) = host.register(name, token, path, icon);
            s.token = Some(t.clone());
            // A new token is answered after 500 ms (§4.1).
            Ok((json!({ "Token": t }), if approved { 0 } else { 500 }))
        }
        "GetIsApprove" => {
            ok(json!({ "Result": s.token.as_deref().is_some_and(|t| host.approved(t)) }))
        }
        "GetCurrentEditMode" => ok(json!({ "EditMode": host.edit_mode() })),
        "GetCurrentModelUID" => match host.current_model(anim) {
            Some(u) => ok(json!({ "ModelUID": u })),
            None => Err(ApiError::InvalidModel),
        },
        "GetCurrentDocumentUID" => match host.current_document() {
            Some(u) => ok(json!({ "DocumentUID": u })),
            None => Err(ApiError::InvalidDocument),
        },
        "GetDocuments" => {
            let docs = host.documents();
            let list = |k: DocKind| -> Value {
                Value::Array(
                    docs.iter()
                        .filter(|d| d.kind == k)
                        .map(|d| doc_json(d, true))
                        .collect(),
                )
            };
            let mut m = Map::new();
            m.insert("PhysicsDocuments".into(), list(DocKind::Physics));
            m.insert("ModelingDocuments".into(), list(DocKind::Modeling));
            if anim {
                m.insert("AnimationDocuments".into(), list(DocKind::Animation));
            }
            ok(Value::Object(m))
        }
        "GetDocument" => {
            let uid = req_str(d, "DocumentUID")?;
            let doc = host
                .documents()
                .into_iter()
                .find(|x| x.uid == uid)
                .ok_or(ApiError::InvalidDocument)?;
            let key = match doc.kind {
                DocKind::Modeling => "ModelingDocuments",
                DocKind::Animation => "AnimationDocuments",
                DocKind::Physics => "PhysicsDocuments",
            };
            ok(json!({ key: [doc_json(&doc, false)] }))
        }
        "GetParameters" => {
            let (m, doc) = (opt_str(d, "ModelUID")?, opt_str(d, "DocumentUID")?);
            let ps = host.parameters(m, doc, anim)?;
            let keys = variant >= 10001;
            ok(json!({ "Parameters": ps.iter().map(|p| param_json(p, keys)).collect::<Vec<_>>() }))
        }
        "GetParameterGroups" => {
            let (m, doc) = (opt_str(d, "ModelUID")?, opt_str(d, "DocumentUID")?);
            let gs = host.groups(m, doc, anim)?;
            ok(
                json!({ "Groups": gs.iter().map(|g| json!({ "GroupUID": g.0, "GroupName": g.1 })).collect::<Vec<_>>() }),
            )
        }
        "GetParameterValues" => {
            let m = req_str(d, "ModelUID")?;
            let ids: Option<Vec<String>> = match d.get("Ids") {
                None | Some(Value::Null) => None,
                Some(Value::Array(a)) => Some(
                    a.iter()
                        .map(|x| x.as_str().map(str::to_owned).ok_or(ApiError::InvalidData))
                        .collect::<Result<_, _>>()?,
                ),
                _ => return Err(ApiError::InvalidData),
            };
            let vals = host.values(m)?;
            let list: Vec<Value> = match ids {
                // Request order; unknown IDs skipped.
                Some(ids) => ids
                    .iter()
                    .filter_map(|id| vals.iter().find(|v| &v.0 == id))
                    .map(|v| json!({ "Id": v.0, "Value": v.1 }))
                    .collect(),
                None => vals
                    .iter()
                    .map(|v| json!({ "Id": v.0, "Value": v.1 }))
                    .collect(),
            };
            ok(json!({ "Parameters": list }))
        }
        "SetParameterValues" => {
            let m = req_str(d, "ModelUID")?;
            let Some(Value::Array(ps)) = d.get("Parameters") else {
                return Err(ApiError::InvalidData);
            };
            let mut vals = Vec::new();
            for p in ps {
                let id = p
                    .get("Id")
                    .and_then(Value::as_str)
                    .ok_or(ApiError::InvalidData)?;
                let v = p
                    .get("Value")
                    .and_then(Value::as_f64)
                    .ok_or(ApiError::InvalidParameter)?;
                vals.push((id.to_owned(), v as f32));
            }
            host.set_values(m, &vals)?;
            ok(json!({}))
        }
        "ClearParameterValues" => {
            let m = req_str(d, "ModelUID")?;
            host.clear_values(m)?;
            ok(json!({}))
        }
        "SendCubismLog" => {
            let msg = req_str(d, "Message")?;
            let warning = match opt_str(d, "Type")? {
                None | Some("info") => false,
                Some("warning") => true,
                Some(_) => return Err(ApiError::InvalidData),
            };
            let display = match d.get("Display") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(b)) => *b,
                _ => return Err(ApiError::InvalidData),
            };
            host.log(warning, msg, display);
            ok(json!({}))
        }
        m if m.starts_with("Notify") => {
            let Some(Value::Bool(on)) = d.get("Enabled") else {
                return Err(ApiError::InvalidData);
            };
            if *on {
                s.notify.insert(m.to_owned());
            } else {
                s.notify.remove(m);
            }
            ok(json!({ "Accepted": true }))
        }
        _ => Err(ApiError::MethodNotFound),
    }
}

/// An event for subscribers of `method` (§4.1 "Events"); `None` when the
/// session isn't subscribed.
pub fn event(s: &Session, method: &str, data: Value, now: u64) -> Option<String> {
    if !s.notify.contains(method) {
        return None;
    }
    let v = s.version.unwrap_or(LATEST);
    Some(envelope(
        version_name(v),
        now,
        None,
        "Event",
        Some(method),
        data,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        set: Vec<(String, f32)>,
        logs: Vec<String>,
        approved: Vec<String>,
    }

    impl Host for Fake {
        fn documents(&self) -> Vec<DocInfo> {
            vec![DocInfo {
                uid: "D1".into(),
                kind: DocKind::Modeling,
                path: "/m.cmo3".into(),
                views: vec![("M1".into(), None)],
            }]
        }
        fn current_document(&self) -> Option<String> {
            Some("D1".into())
        }
        fn current_model(&self, _: bool) -> Option<String> {
            Some("M1".into())
        }
        fn edit_mode(&self) -> &'static str {
            "Modeling"
        }
        fn parameters(
            &self,
            _: Option<&str>,
            _: Option<&str>,
            _: bool,
        ) -> Result<Vec<ParamInfo>, ApiError> {
            Ok(vec![ParamInfo {
                id: "ParamAngleX".into(),
                name: "Angle X".into(),
                group_uid: "G".into(),
                default: 0.0,
                max: 30.0,
                min: -30.0,
                repeat: false,
                kind: 0,
                keys: vec![-30.0, 0.0, 30.0],
            }])
        }
        fn groups(
            &self,
            _: Option<&str>,
            _: Option<&str>,
            _: bool,
        ) -> Result<Vec<(String, String)>, ApiError> {
            Ok(vec![("G".into(), "Face".into())])
        }
        fn values(&self, m: &str) -> Result<Vec<(String, f32)>, ApiError> {
            if m != "M1" {
                return Err(ApiError::InvalidModel);
            }
            Ok(vec![("A".into(), 1.0), ("B".into(), 2.0)])
        }
        fn set_values(&mut self, _: &str, v: &[(String, f32)]) -> Result<(), ApiError> {
            self.set.extend_from_slice(v);
            Ok(())
        }
        fn clear_values(&mut self, _: &str) -> Result<(), ApiError> {
            self.set.clear();
            Ok(())
        }
        fn log(&mut self, _: bool, m: &str, _: bool) {
            self.logs.push(m.into());
        }
        fn register(
            &mut self,
            _: &str,
            token: Option<&str>,
            _: Option<&str>,
            _: Option<&str>,
        ) -> (String, bool) {
            match token {
                Some(t) if self.approved.iter().any(|a| a == t) => (t.into(), true),
                _ => ("T1".into(), false),
            }
        }
        fn approved(&self, t: &str) -> bool {
            self.approved.iter().any(|a| a == t)
        }
    }

    fn req(v: &str, method: &str, data: Value) -> String {
        json!({ "Version": v, "Timestamp": 0, "RequestId": "r1", "Type": "Request", "Method": method, "Data": data }).to_string()
    }

    fn parse(o: &[Outgoing]) -> Value {
        serde_json::from_str(&o[0].text).unwrap()
    }

    #[test]
    fn protocol() {
        let mut h = Fake::default();
        let mut s = Session::default();
        // Bad JSON; unknown version; method not found.
        assert_eq!(
            parse(&handle("{", &mut s, &mut h, 5))["Data"]["ErrorType"],
            "InvalidJson"
        );
        assert_eq!(
            parse(&handle(
                &req("2.0.0", "GetAPIVersion", json!({})),
                &mut s,
                &mut h,
                5
            ))["Data"]["ErrorType"],
            "UnsupportedVersion"
        );
        // GetAPIVersion did not exist in 0.9.0.
        assert_eq!(
            parse(&handle(
                &req("0.9.0", "GetAPIVersion", json!({})),
                &mut s,
                &mut h,
                5
            ))["Data"]["ErrorType"],
            "UnsupportedVersion"
        );
        let r = parse(&handle(
            &req("1.0.0", "GetAPIVersion", json!({})),
            &mut s,
            &mut h,
            5,
        ));
        assert_eq!(r["Data"]["Version"], "1.0.1");
        assert_eq!(r["Version"], "0.9.1", "served by the 0.9.1 variant");
        assert_eq!(r["Type"], "Response");
        assert_eq!(r["RequestId"], "r1");
        // Key order of the envelope.
        let o = handle(&req("1.0.1", "GetAPIVersion", json!({})), &mut s, &mut h, 5);
        assert!(o[0].text.starts_with("{\"Version\":\"0.9.1\",\"Timestamp\":5,\"RequestId\":\"r1\",\"Type\":\"Response\",\"Method\""));
        // Not approved yet.
        assert_eq!(
            parse(&handle(
                &req("1.0.1", "GetParameters", json!({})),
                &mut s,
                &mut h,
                5
            ))["Data"]["ErrorType"],
            "PluginNotRegistered"
        );
        let o = handle(
            &req("1.0.1", "RegisterPlugin", json!({ "Name": "Tracker" })),
            &mut s,
            &mut h,
            5,
        );
        assert_eq!(o[0].delay_ms, 500);
        assert_eq!(parse(&o)["Data"]["Token"], "T1");
        assert_eq!(
            parse(&handle(
                &req("1.0.1", "GetIsApprove", json!({})),
                &mut s,
                &mut h,
                5
            ))["Data"]["Result"],
            false
        );
        h.approved.push("T1".into());
        // Parameters: 1.0.1 adds Keyform; 1.0.0 session is served by 0.9.1.
        let r = parse(&handle(
            &req("1.0.1", "GetParameters", json!({})),
            &mut s,
            &mut h,
            5,
        ));
        assert_eq!(r["Version"], "1.0.1");
        assert_eq!(r["Data"]["Parameters"][0]["Keyform"][2]["Value"], 30.0);
        let r = parse(&handle(
            &req("1.0.0", "GetParameters", json!({})),
            &mut s,
            &mut h,
            5,
        ));
        assert!(r["Data"]["Parameters"][0].get("Keyform").is_none());
        // Values in request order, unknown skipped; validation.
        let r = parse(&handle(
            &req(
                "1.0.1",
                "GetParameterValues",
                json!({ "ModelUID": "M1", "Ids": ["B", "X", "A"] }),
            ),
            &mut s,
            &mut h,
            5,
        ));
        assert_eq!(
            r["Data"]["Parameters"],
            json!([{ "Id": "B", "Value": 2.0 }, { "Id": "A", "Value": 1.0 }])
        );
        assert_eq!(
            parse(&handle(
                &req("1.0.1", "GetParameterValues", json!({})),
                &mut s,
                &mut h,
                5
            ))["Data"]["ErrorType"],
            "InvalidData"
        );
        let r = parse(&handle(
            &req(
                "1.0.1",
                "SetParameterValues",
                json!({ "ModelUID": "M1", "Parameters": [{ "Id": "A", "Value": "x" }] }),
            ),
            &mut s,
            &mut h,
            5,
        ));
        assert_eq!(r["Data"]["ErrorType"], "InvalidParameter");
        handle(
            &req(
                "1.0.1",
                "SetParameterValues",
                json!({ "ModelUID": "M1", "Parameters": [{ "Id": "A", "Value": 0.5 }] }),
            ),
            &mut s,
            &mut h,
            5,
        );
        assert_eq!(h.set, vec![("A".to_owned(), 0.5)]);
        // Documents: 0.9.0 has no AnimationDocuments; GetDocument drops the UID.
        let r = parse(&handle(
            &req("0.9.0", "GetDocuments", json!({})),
            &mut s,
            &mut h,
            5,
        ));
        assert!(r["Data"].get("AnimationDocuments").is_none());
        assert_eq!(
            r["Data"]["ModelingDocuments"][0]["Views"][0]["ModelUID"],
            "M1"
        );
        let r = parse(&handle(
            &req("1.0.1", "GetDocument", json!({ "DocumentUID": "D1" })),
            &mut s,
            &mut h,
            5,
        ));
        assert!(r["Data"]["ModelingDocuments"][0]
            .get("DocumentUID")
            .is_none());
        // Subscriptions and events.
        let r = parse(&handle(
            &req("1.0.1", "NotifyChangeEditMode", json!({ "Enabled": true })),
            &mut s,
            &mut h,
            5,
        ));
        assert_eq!(r["Data"]["Accepted"], true);
        let e: Value = serde_json::from_str(
            &event(
                &s,
                "NotifyChangeEditMode",
                json!({ "EditMode": "Animation" }),
                9,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(e["Type"], "Event");
        assert!(e.get("RequestId").is_none());
        assert!(event(&s, "NotifyMocFileExported", json!({}), 9).is_none());
        handle(
            &req(
                "1.0.1",
                "SendCubismLog",
                json!({ "Message": "hi", "Type": "warning" }),
            ),
            &mut s,
            &mut h,
            5,
        );
        assert_eq!(h.logs, vec!["hi"]);
        // SetGlobalVersion pins the session.
        handle(
            &req("1.0.1", "SetGlobalVersion", json!({ "Version": "0.9.0" })),
            &mut s,
            &mut h,
            5,
        );
        assert_eq!(
            parse(&handle(
                &req("1.0.1", "ClearParameterValues", json!({ "ModelUID": "M1" })),
                &mut s,
                &mut h,
                5
            ))["Data"]["ErrorType"],
            "UnsupportedVersion"
        );
    }
}
