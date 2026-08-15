use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayInfo {
    pub lowerdir: Option<String>,
    pub upperdir: Option<String>,
    pub workdir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapEntry {
    pub first: u32,
    pub lower_first: u32,
    pub count: u32,
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

    push_line(
        &mut out,
        "inode",
        &format!("{}  dev {}:{}", r.inode, r.dev_major, r.dev_minor),
    );
    push_line(
        &mut out,
        "owner",
        &format!(
            "disk {}:{}  mapped {}:{}",
            r.disk_uid,
            r.disk_gid,
            format_mapped_id(r.mapped_uid),
            format_mapped_id(r.mapped_gid)
        ),
    );
    push_line(
        &mut out,
        "mount",
        &format!(
            "id={}  {}  {}  {}",
            r.mount_id, r.mount_fstype, r.mount_target, r.mount_flags
        ),
    );
    push_indent(&mut out, "bind", &r.mount_bind);

    if let Some(ov) = &r.overlay {
        if let Some(lower) = &ov.lowerdir {
            push_indent(&mut out, "lower", lower);
        }
        if let Some(upper) = &ov.upperdir {
            push_indent(&mut out, "upper", upper);
        }
        if let Some(work) = &ov.workdir {
            push_indent(&mut out, "work", work);
        }
        push_indent(&mut out, "layer", "cannot prove upper vs lower");
    }

    out
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

    fn sample() -> InspectResult {
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

    #[test]
    fn text_layout_has_sections_and_owner() {
        let text = format_inspect_text(&sample());
        assert!(text.contains("pid     4821\n"));
        assert!(text.contains("path    /data/config\n"));
        assert!(text.contains("inode   88421  dev 0:92\n"));
        assert!(text.contains("owner   disk 100000:100000  mapped 0:0\n"));
        assert!(text.contains("mount   id=892  overlay  /data  rw,relatime\n"));
        assert!(text.contains("        lower /lower\n"));
        assert!(text.contains("        layer cannot prove upper vs lower\n"));
        // blank line between process context and creds, and before inode block
        assert!(text.contains("ns.user user:[4026531837]\n\nuid"));
        assert!(text.contains("gid.map 0 100000 65536\n\ninode"));
    }
}
