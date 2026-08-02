// /proc/<pid>/mountinfo line → fields we care about for covering mounts later

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountInfo {
    pub id: u32,
    pub parent: u32,
    pub root: String,
    pub target: String,
    pub fstype: String,
    pub source: String,
    pub options: String,
}

pub fn parse_mountinfo(text: &str) -> Result<Vec<MountInfo>, String> {
    let mut mounts = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        mounts.push(
            parse_mountinfo_line(line)
                .map_err(|e| format!("mountinfo line {}: {e}", i + 1))?,
        );
    }
    Ok(mounts)
}

pub fn find_mount(mounts: &[MountInfo], id: u64) -> Option<&MountInfo> {
    mounts.iter().find(|m| u64::from(m.id) == id)
}

fn parse_mountinfo_line(line: &str) -> Result<MountInfo, String> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let sep = fields
        .iter()
        .position(|f| *f == "-")
        .ok_or_else(|| "missing '-' separator".to_string())?;

    // id parent maj:min root target options [optional...] - fstype source super
    if sep < 6 {
        return Err(format!("expected at least 6 fields before '-', got {sep}"));
    }
    if fields.len() - sep < 4 {
        return Err("expected fstype, source, and super options after '-'".into());
    }

    let id = fields[0]
        .parse()
        .map_err(|_| format!("bad mount id `{}`", fields[0]))?;
    let parent = fields[1]
        .parse()
        .map_err(|_| format!("bad parent id `{}`", fields[1]))?;

    Ok(MountInfo {
        id,
        parent,
        root: unescape_mount_field(fields[3]),
        target: unescape_mount_field(fields[4]),
        options: fields[5].to_string(),
        fstype: fields[sep + 1].to_string(),
        source: unescape_mount_field(fields[sep + 2]),
        // super options (fields[sep + 3]) ignored for now
    })
}

// mountinfo escapes spaces and a few controls as \ooo octal
fn unescape_mount_field(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() {
            let b1 = bytes[i + 1];
            let b2 = bytes[i + 2];
            let b3 = bytes[i + 3];
            if matches!(b1, b'0'..=b'7') && matches!(b2, b'0'..=b'7') && matches!(b3, b'0'..=b'7')
            {
                let val = ((b1 - b'0') << 6) | ((b2 - b'0') << 3) | (b3 - b'0');
                out.push(val as char);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bind_mount() {
        // bind of a host file onto /etc/resolv.conf (root field is the bind source path)
        let text = "892 821 8:2 /var/lib/docker/containers/abc/resolv.conf /etc/resolv.conf rw,relatime - ext4 /dev/sda1 rw\n";
        let mounts = parse_mountinfo(text).unwrap();
        assert_eq!(
            mounts,
            vec![MountInfo {
                id: 892,
                parent: 821,
                root: "/var/lib/docker/containers/abc/resolv.conf".into(),
                target: "/etc/resolv.conf".into(),
                fstype: "ext4".into(),
                source: "/dev/sda1".into(),
                options: "rw,relatime".into(),
            }]
        );
    }

    #[test]
    fn parse_overlay_mount() {
        let text = "\
915 821 0:92 / /data rw,relatime - overlay overlay rw,lowerdir=/usr/lib/myapp,upperdir=/var/lib/containers/diff,workdir=/var/lib/containers/work
";
        let mounts = parse_mountinfo(text).unwrap();
        assert_eq!(
            mounts,
            vec![MountInfo {
                id: 915,
                parent: 821,
                root: "/".into(),
                target: "/data".into(),
                fstype: "overlay".into(),
                source: "overlay".into(),
                options: "rw,relatime".into(),
            }]
        );
    }

    #[test]
    fn parse_bind_and_overlay_dump() {
        let text = "\
36 35 98:0 / / rw,relatime shared:1 - ext4 /dev/root rw,errors=continue
892 36 8:2 /host/config.yaml /app/config.yaml rw,relatime - ext4 /dev/sda1 rw
915 36 0:92 / /data rw,relatime - overlay overlay rw,lowerdir=/lower,upperdir=/upper,workdir=/work
";
        let mounts = parse_mountinfo(text).unwrap();
        assert_eq!(mounts.len(), 3);
        assert_eq!(mounts[1].target, "/app/config.yaml");
        assert_eq!(mounts[1].root, "/host/config.yaml");
        assert_eq!(mounts[2].fstype, "overlay");
        assert_eq!(mounts[2].target, "/data");
    }

    #[test]
    fn unescape_space_in_path() {
        let text = "10 1 8:1 /foo\\040bar /mnt/foo\\040bar rw - ext4 /dev/sda1 rw\n";
        let mounts = parse_mountinfo(text).unwrap();
        assert_eq!(mounts[0].root, "/foo bar");
        assert_eq!(mounts[0].target, "/mnt/foo bar");
    }
}
