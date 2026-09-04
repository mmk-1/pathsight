use serde::Serialize;
use std::path::{Path, PathBuf};

const ERR_MISSING: &str = "path missing (ENOENT)";
const ERR_ACCESS: &str = "permission denied (EACCES)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayInfo {
    pub lowerdir: Option<String>,
    pub upperdir: Option<String>,
    pub workdir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MapEntry {
    pub first: u32,
    pub lower_first: u32,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathDetails {
    pub inode: u64,
    pub dev_major: u32,
    pub dev_minor: u32,
    pub disk_uid: u32,
    pub disk_gid: u32,
    pub mapped_uid: Option<u32>,
    pub mapped_gid: Option<u32>,
    pub mount_id: u32,
    pub mount_fstype: String,
    pub mount_target: String,
    pub mount_bind: String,
    pub mount_flags: String,
    pub overlay: Option<OverlayInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathOutcome {
    Resolved(PathDetails),
    /// ENOENT — path does not exist in that process's view
    Missing,
    /// EACCES / EPERM — cannot open the path
    AccessDenied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectResult {
    pub pid: u32,
    pub path: PathBuf,
    pub root: PathBuf,
    pub cwd: PathBuf,
    pub ns_mnt: PathBuf,
    pub ns_user: PathBuf,
    pub uid: [u32; 4],
    pub gid: [u32; 4],
    pub uid_map: Vec<MapEntry>,
    pub gid_map: Vec<MapEntry>,
    pub outcome: PathOutcome,
}

pub fn format_inspect_text(r: &InspectResult) -> String {
    let mut out = String::new();

    push_line(&mut out, "pid", &r.pid.to_string());
    push_line(&mut out, "path", &r.path.display().to_string());
    push_line(&mut out, "root", &r.root.display().to_string());
    push_line(&mut out, "cwd", &r.cwd.display().to_string());
    push_line(&mut out, "ns.mnt", &r.ns_mnt.display().to_string());
    push_line(&mut out, "ns.user", &r.ns_user.display().to_string());
    out.push('\n');

    push_line(
        &mut out,
        "uid",
        &format!(
            "r={} e={} s={} fs={}",
            r.uid[0], r.uid[1], r.uid[2], r.uid[3]
        ),
    );
    push_line(
        &mut out,
        "gid",
        &format!(
            "r={} e={} s={} fs={}",
            r.gid[0], r.gid[1], r.gid[2], r.gid[3]
        ),
    );
    push_id_map(&mut out, "uid.map", &r.uid_map);
    push_id_map(&mut out, "gid.map", &r.gid_map);
    out.push('\n');

    match &r.outcome {
        PathOutcome::Resolved(p) => {
            push_line(
                &mut out,
                "inode",
                &format!("{}  dev {}:{}", p.inode, p.dev_major, p.dev_minor),
            );
            push_line(
                &mut out,
                "owner",
                &format!(
                    "disk {}:{}  mapped {}:{}",
                    p.disk_uid,
                    p.disk_gid,
                    format_mapped_id(p.mapped_uid),
                    format_mapped_id(p.mapped_gid)
                ),
            );
            push_line(
                &mut out,
                "mount",
                &format!(
                    "id={}  {}  {}  {}",
                    p.mount_id, p.mount_fstype, p.mount_target, p.mount_flags
                ),
            );
            push_indent(&mut out, "bind", &p.mount_bind);

            if let Some(ov) = &p.overlay {
                if let Some(lower) = &ov.lowerdir {
                    push_indent(&mut out, "lower", lower);
                }
                if let Some(upper) = &ov.upperdir {
                    push_indent(&mut out, "upper", upper);
                }
                if let Some(work) = &ov.workdir {
                    push_indent(&mut out, "work", work);
                }
            }

            for msg in path_warnings(&r.path, p) {
                push_line(&mut out, "warning", &msg);
            }
        }
        PathOutcome::Missing => {
            push_line(&mut out, "error", ERR_MISSING);
        }
        PathOutcome::AccessDenied => {
            push_line(&mut out, "error", ERR_ACCESS);
        }
    }

    out
}

pub fn format_inspect_json(r: &InspectResult) -> String {
    serde_json::to_string_pretty(&inspect_json(r)).expect("inspect result is serializable")
}

#[derive(Serialize)]
struct IdSet {
    r: u32,
    e: u32,
    s: u32,
    fs: u32,
}

#[derive(Serialize)]
struct DevJson {
    major: u32,
    minor: u32,
}

#[derive(Serialize)]
struct OwnerJson {
    disk_uid: u32,
    disk_gid: u32,
    mapped_uid: Option<u32>,
    mapped_gid: Option<u32>,
}

#[derive(Serialize)]
struct MountJson<'a> {
    id: u32,
    fstype: &'a str,
    target: &'a str,
    bind: &'a str,
    flags: &'a str,
}

#[derive(Serialize)]
struct OverlayJson<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    lowerdir: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    upperdir: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workdir: Option<&'a str>,
}

#[derive(Serialize)]
struct InspectJson<'a> {
    pid: u32,
    path: String,
    root: String,
    cwd: String,
    ns_mnt: String,
    ns_user: String,
    uid: IdSet,
    gid: IdSet,
    uid_map: &'a [MapEntry],
    gid_map: &'a [MapEntry],
    #[serde(skip_serializing_if = "Option::is_none")]
    inode: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dev: Option<DevJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<OwnerJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mount: Option<MountJson<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    overlay: Option<OverlayJson<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

fn inspect_json(r: &InspectResult) -> InspectJson<'_> {
    let mut j = InspectJson {
        pid: r.pid,
        path: r.path.display().to_string(),
        root: r.root.display().to_string(),
        cwd: r.cwd.display().to_string(),
        ns_mnt: r.ns_mnt.display().to_string(),
        ns_user: r.ns_user.display().to_string(),
        uid: id_set(r.uid),
        gid: id_set(r.gid),
        uid_map: &r.uid_map,
        gid_map: &r.gid_map,
        inode: None,
        dev: None,
        owner: None,
        mount: None,
        overlay: None,
        error: None,
        warnings: Vec::new(),
    };

    match &r.outcome {
        PathOutcome::Resolved(p) => {
            j.inode = Some(p.inode);
            j.dev = Some(DevJson {
                major: p.dev_major,
                minor: p.dev_minor,
            });
            j.owner = Some(OwnerJson {
                disk_uid: p.disk_uid,
                disk_gid: p.disk_gid,
                mapped_uid: p.mapped_uid,
                mapped_gid: p.mapped_gid,
            });
            j.mount = Some(MountJson {
                id: p.mount_id,
                fstype: &p.mount_fstype,
                target: &p.mount_target,
                bind: &p.mount_bind,
                flags: &p.mount_flags,
            });
            if let Some(ov) = &p.overlay {
                j.overlay = Some(OverlayJson {
                    lowerdir: ov.lowerdir.as_deref(),
                    upperdir: ov.upperdir.as_deref(),
                    workdir: ov.workdir.as_deref(),
                });
            }
            j.warnings = path_warnings(&r.path, p);
        }
        PathOutcome::Missing => j.error = Some(ERR_MISSING),
        PathOutcome::AccessDenied => j.error = Some(ERR_ACCESS),
    }

    j
}

fn id_set(ids: [u32; 4]) -> IdSet {
    IdSet {
        r: ids[0],
        e: ids[1],
        s: ids[2],
        fs: ids[3],
    }
}

pub fn path_warnings(path: &Path, p: &PathDetails) -> Vec<String> {
    let mut warnings = Vec::new();

    if mount_has_option(&p.mount_flags, "ro") {
        warnings.push("mount is read-only".into());
    }

    if path_is_mount_target(path, &p.mount_target) {
        warnings.push("path is covered by a mount".into());
    }

    if p.mapped_uid.is_none() {
        warnings.push("disk uid is unmapped in this user namespace".into());
    }
    if p.mapped_gid.is_none() {
        warnings.push("disk gid is unmapped in this user namespace".into());
    }

    if p.overlay.is_some() {
        warnings.push("cannot prove overlay upper vs lower".into());
    }

    warnings
}

fn mount_has_option(options: &str, name: &str) -> bool {
    options.split(',').any(|opt| opt == name)
}

fn path_is_mount_target(path: &Path, mount_target: &str) -> bool {
    norm_path(path) == norm_str(mount_target)
}

fn norm_path(path: &Path) -> String {
    norm_str(&path.to_string_lossy()).to_string()
}

fn norm_str(s: &str) -> &str {
    if s.len() > 1 {
        s.trim_end_matches('/')
    } else {
        s
    }
}

fn push_line(out: &mut String, key: &str, value: &str) {
    out.push_str(&format!("{key:<8}{value}\n"));
}

fn push_indent(out: &mut String, key: &str, value: &str) {
    out.push_str(&format!("        {key:<6}{value}\n"));
}

fn push_id_map(out: &mut String, label: &str, entries: &[MapEntry]) {
    if entries.is_empty() {
        push_line(out, label, "(empty)");
        return;
    }
    for e in entries {
        push_line(
            out,
            label,
            &format!("{} {} {}", e.first, e.lower_first, e.count),
        );
    }
}

fn format_mapped_id(id: Option<u32>) -> String {
    match id {
        Some(id) => id.to_string(),
        None => "unmapped".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_context() -> InspectResult {
        InspectResult {
            pid: 4821,
            path: PathBuf::from("/data/config"),
            root: PathBuf::from("/"),
            cwd: PathBuf::from("/"),
            ns_mnt: PathBuf::from("mnt:[4026532680]"),
            ns_user: PathBuf::from("user:[4026531837]"),
            uid: [1000, 1000, 1000, 1000],
            gid: [1000, 1000, 1000, 1000],
            uid_map: vec![MapEntry {
                first: 0,
                lower_first: 100000,
                count: 65536,
            }],
            gid_map: vec![MapEntry {
                first: 0,
                lower_first: 100000,
                count: 65536,
            }],
            outcome: PathOutcome::Missing,
        }
    }

    fn sample_details() -> PathDetails {
        PathDetails {
            inode: 88421,
            dev_major: 0,
            dev_minor: 92,
            disk_uid: 100000,
            disk_gid: 100000,
            mapped_uid: Some(0),
            mapped_gid: Some(0),
            mount_id: 892,
            mount_fstype: "overlay".into(),
            mount_target: "/data".into(),
            mount_bind: "/".into(),
            mount_flags: "rw,relatime".into(),
            overlay: Some(OverlayInfo {
                lowerdir: Some("/lower".into()),
                upperdir: Some("/upper".into()),
                workdir: Some("/work".into()),
            }),
        }
    }

    fn sample() -> InspectResult {
        let mut r = sample_context();
        r.outcome = PathOutcome::Resolved(sample_details());
        r
    }

    #[test]
    fn text_layout_has_sections_and_owner() {
        let text = format_inspect_text(&sample());
        assert!(text.contains("pid     4821\n"));
        assert!(text.contains("path    /data/config\n"));
        assert!(text.contains("inode   88421  dev 0:92\n"));
        assert!(text.contains("owner   disk 100000:100000  mapped 0:0\n"));
        assert!(text.contains("mount   id=892  overlay  /data  rw,relatime\n"));
        assert!(text.contains("        lower /lower\n"));
        assert!(text.contains("warning cannot prove overlay upper vs lower\n"));
        assert!(!text.contains("        layer "));
        // blank line between process context and creds, and before inode block
        assert!(text.contains("ns.user user:[4026531837]\n\nuid"));
        assert!(text.contains("gid.map 0 100000 65536\n\ninode"));
    }

    #[test]
    fn text_error_path_missing() {
        let text = format_inspect_text(&sample_context());
        assert!(text.contains("pid     4821\n"));
        assert!(text.contains("path    /data/config\n"));
        assert!(text.contains("error   path missing (ENOENT)\n"));
        assert!(!text.contains("inode"));
    }

    #[test]
    fn text_error_access_denied() {
        let mut r = sample_context();
        r.outcome = PathOutcome::AccessDenied;
        let text = format_inspect_text(&r);
        assert!(text.contains("error   permission denied (EACCES)\n"));
        assert!(!text.contains("inode"));
    }

    #[test]
    fn warnings_read_only() {
        let mut p = sample_details();
        p.mount_flags = "ro,relatime".into();
        p.overlay = None;
        let w = path_warnings(Path::new("/data/config"), &p);
        assert!(w.iter().any(|m| m == "mount is read-only"));
    }

    #[test]
    fn warnings_covering_mount() {
        let mut p = sample_details();
        p.mount_target = "/data/config".into();
        p.overlay = None;
        let w = path_warnings(Path::new("/data/config"), &p);
        assert!(w.iter().any(|m| m == "path is covered by a mount"));
    }

    #[test]
    fn warnings_uid_gid_unmapped() {
        let mut p = sample_details();
        p.mapped_uid = None;
        p.mapped_gid = None;
        p.overlay = None;
        let w = path_warnings(Path::new("/data/config"), &p);
        assert!(w.iter().any(|m| m == "disk uid is unmapped in this user namespace"));
        assert!(w.iter().any(|m| m == "disk gid is unmapped in this user namespace"));
    }

    #[test]
    fn warnings_ro_not_confused_with_substring() {
        let mut p = sample_details();
        p.mount_flags = "rw,errors=remount-ro".into();
        p.overlay = None;
        let w = path_warnings(Path::new("/data/config"), &p);
        assert!(!w.iter().any(|m| m == "mount is read-only"));
    }

    fn parse_json(r: &InspectResult) -> serde_json::Value {
        serde_json::from_str(&format_inspect_json(r)).unwrap()
    }

    #[test]
    fn json_resolved_mirrors_inspect_fields() {
        let v = parse_json(&sample());
        assert_eq!(v["pid"], 4821);
        assert_eq!(v["path"], "/data/config");
        assert_eq!(v["root"], "/");
        assert_eq!(v["ns_mnt"], "mnt:[4026532680]");
        assert_eq!(v["uid"]["r"], 1000);
        assert_eq!(v["uid_map"][0]["lower_first"], 100000);
        assert_eq!(v["inode"], 88421);
        assert_eq!(v["dev"]["major"], 0);
        assert_eq!(v["dev"]["minor"], 92);
        assert_eq!(v["owner"]["disk_uid"], 100000);
        assert_eq!(v["owner"]["mapped_uid"], 0);
        assert_eq!(v["mount"]["id"], 892);
        assert_eq!(v["mount"]["fstype"], "overlay");
        assert_eq!(v["mount"]["bind"], "/");
        assert_eq!(v["overlay"]["lowerdir"], "/lower");
        assert_eq!(v["overlay"]["upperdir"], "/upper");
        assert_eq!(v["overlay"]["workdir"], "/work");
        let warnings = v["warnings"].as_array().unwrap();
        assert!(warnings.iter().any(|w| w == "cannot prove overlay upper vs lower"));
        assert!(v.get("error").is_none());
    }

    #[test]
    fn json_missing_has_error_no_inode() {
        let v = parse_json(&sample_context());
        assert_eq!(v["pid"], 4821);
        assert_eq!(v["path"], "/data/config");
        assert_eq!(v["error"], ERR_MISSING);
        assert!(v.get("inode").is_none());
        assert!(v.get("mount").is_none());
        assert!(v.get("warnings").is_none());
    }

    #[test]
    fn json_access_denied() {
        let mut r = sample_context();
        r.outcome = PathOutcome::AccessDenied;
        let v = parse_json(&r);
        assert_eq!(v["error"], ERR_ACCESS);
        assert!(v.get("inode").is_none());
    }

    #[test]
    fn json_unmapped_uid_is_null() {
        let mut r = sample();
        if let PathOutcome::Resolved(p) = &mut r.outcome {
            p.mapped_uid = None;
            p.overlay = None;
        }
        let v = parse_json(&r);
        assert!(v["owner"]["mapped_uid"].is_null());
        assert_eq!(v["owner"]["mapped_gid"], 0);
        let warnings = v["warnings"].as_array().unwrap();
        assert!(warnings.iter().any(|w| w == "disk uid is unmapped in this user namespace"));
        assert!(v.get("overlay").is_none());
    }
}
