// SPDX-License-Identifier: MIT

//! GTK-free local webcam inventory helpers.
//!
//! BigLinux media apps need a shared, lightweight way to discover local
//! `/dev/videoN` nodes without pulling in GStreamer, GTK, V4L2 format probing,
//! or application-specific camera descriptors. Higher-level apps still own
//! capability checks and pipeline construction.

use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(feature = "webcam-v4l2")]
use std::process::Command;

#[cfg(feature = "webcam-v4l2")]
const V4L2_CTL: &str = "v4l2-ctl";

/// Local Linux video device discovered through `/sys/class/video4linux`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoDevice {
    /// Device node path, normally `/dev/videoN`.
    pub path: PathBuf,
    /// Kernel video node name, for example `video0`.
    pub node_name: String,
    /// Human-readable device name from sysfs, or the node name as fallback.
    pub display_name: String,
}

impl VideoDevice {
    /// User-facing label for this device.
    #[must_use]
    pub fn label(&self) -> &str {
        if self.display_name.is_empty() {
            &self.node_name
        } else {
            &self.display_name
        }
    }
}

/// Local V4L2 video-capture device with kernel capabilities confirmed.
#[cfg(feature = "webcam-v4l2")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoCaptureDevice {
    /// Device node path, normally `/dev/videoN`.
    pub path: PathBuf,
    /// Kernel video node name, for example `video0`.
    pub node_name: String,
    /// Human-readable name from V4L2 card/sysfs, or the node name fallback.
    pub display_name: String,
    /// V4L2 driver name, for example `uvcvideo`.
    pub driver: String,
    /// V4L2 bus info, commonly a USB or PCI identifier.
    pub bus_info: String,
}

#[cfg(feature = "webcam-v4l2")]
impl VideoCaptureDevice {
    /// User-facing label for this capture device.
    #[must_use]
    pub fn label(&self) -> &str {
        if self.display_name.is_empty() {
            &self.node_name
        } else {
            &self.display_name
        }
    }
}

/// List local `/dev/videoN` nodes advertised by sysfs.
///
/// The result is sorted by numeric video node index. Entries that do not look
/// exactly like `videoN` are ignored to avoid path traversal or argv surprises
/// when consumers pass the path to native camera tools.
#[must_use]
pub fn list_video_devices() -> Vec<VideoDevice> {
    list_video_devices_from_roots(Path::new("/sys/class/video4linux"), Path::new("/dev"))
}

/// List local V4L2 capture-capable webcam devices.
///
/// This filters the raw sysfs list through `VIDIOC_QUERYCAP`, rejects nodes
/// that are not `VIDEO_CAPTURE`, and excludes v4l2loopback devices. Use this
/// for recording/camera pipelines; use [`list_video_devices`] only for raw
/// node inventory or diagnostics.
#[cfg(feature = "webcam-v4l2")]
#[must_use]
pub fn list_video_capture_devices() -> Vec<VideoCaptureDevice> {
    list_video_devices()
        .into_iter()
        .filter_map(video_capture_device_from_video_device)
        .collect()
}

fn list_video_devices_from_roots(sys_video_class: &Path, dev_root: &Path) -> Vec<VideoDevice> {
    let Ok(entries) = fs::read_dir(sys_video_class) else {
        return Vec::new();
    };
    let mut devices = entries
        .filter_map(Result::ok)
        .filter_map(|entry| video_device_from_sysfs_entry(&entry.path(), dev_root))
        .collect::<Vec<_>>();
    devices.sort_by_key(|device| video_node_index(&device.node_name).unwrap_or(u32::MAX));
    devices
}

fn video_device_from_sysfs_entry(sysfs_path: &Path, dev_root: &Path) -> Option<VideoDevice> {
    let node_name = sysfs_path.file_name()?.to_str()?.to_string();
    video_node_index(&node_name)?;
    let device_path = dev_root.join(&node_name);
    if !device_path.exists() {
        return None;
    }
    let display_name = read_sysfs_device_name(sysfs_path).unwrap_or_else(|| node_name.clone());
    Some(VideoDevice {
        path: device_path,
        node_name,
        display_name,
    })
}

fn read_sysfs_device_name(sysfs_path: &Path) -> Option<String> {
    let name = fs::read_to_string(sysfs_path.join("name")).ok()?;
    let trimmed = name.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn video_node_index(node_name: &str) -> Option<u32> {
    let suffix = node_name.strip_prefix("video")?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    suffix.parse().ok()
}

#[cfg(feature = "webcam-v4l2")]
fn video_capture_device_from_video_device(device: VideoDevice) -> Option<VideoCaptureDevice> {
    let v4l2_capabilities = query_v4l2_capture_capabilities(&device.path)?;
    video_capture_device_from_v4l2_capabilities(device, &v4l2_capabilities)
}

#[cfg(feature = "webcam-v4l2")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct V4l2CaptureCapabilities {
    driver: String,
    card: String,
    bus_identifier: String,
    has_video_capture: bool,
}

#[cfg(feature = "webcam-v4l2")]
fn query_v4l2_capture_capabilities(path: &Path) -> Option<V4l2CaptureCapabilities> {
    let output = Command::new(V4L2_CTL)
        .arg("--device")
        .arg(path)
        .arg("--all")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_v4l2_ctl_all(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(feature = "webcam-v4l2")]
fn parse_v4l2_ctl_all(output: &str) -> Option<V4l2CaptureCapabilities> {
    let mut driver = String::new();
    let mut card = String::new();
    let mut bus_identifier = String::new();
    let mut has_video_capture = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(value) = label_value(trimmed, "Driver name") {
            driver = value.to_owned();
        } else if let Some(value) = label_value(trimmed, "Card type") {
            card = value.to_owned();
        } else if let Some(value) = label_value(trimmed, "Bus info") {
            bus_identifier = value.to_owned();
        } else if trimmed == "Video Capture" {
            has_video_capture = true;
        }
    }

    (!driver.is_empty()).then_some(V4l2CaptureCapabilities {
        driver,
        card,
        bus_identifier,
        has_video_capture,
    })
}

#[cfg(feature = "webcam-v4l2")]
fn label_value<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let (found_label, value) = line.split_once(':')?;
    (found_label.trim() == label)
        .then(|| value.trim())
        .filter(|value| !value.is_empty())
}

#[cfg(feature = "webcam-v4l2")]
fn video_capture_device_from_v4l2_capabilities(
    device: VideoDevice,
    v4l2_capabilities: &V4l2CaptureCapabilities,
) -> Option<VideoCaptureDevice> {
    if is_loopback_driver(&v4l2_capabilities.driver) || !v4l2_capabilities.has_video_capture {
        return None;
    }
    Some(VideoCaptureDevice {
        path: device.path,
        node_name: device.node_name,
        display_name: if v4l2_capabilities.card.is_empty() {
            device.display_name
        } else {
            v4l2_capabilities.card.clone()
        },
        driver: v4l2_capabilities.driver.clone(),
        bus_info: v4l2_capabilities.bus_identifier.clone(),
    })
}

#[cfg(feature = "webcam-v4l2")]
fn is_loopback_driver(driver: &str) -> bool {
    matches!(driver, "v4l2 loopback" | "v4l2loopback")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_node_index_accepts_only_video_number_nodes() {
        assert_eq!(video_node_index("video0"), Some(0));
        assert_eq!(video_node_index("video42"), Some(42));
        assert_eq!(video_node_index("video"), None);
        assert_eq!(video_node_index("video0x"), None);
        assert_eq!(video_node_index("../video0"), None);
        assert_eq!(video_node_index("-video0"), None);
    }

    #[test]
    fn list_video_devices_reads_names_and_sorts_by_node_index() {
        let root = unique_test_dir("big-os-kit-webcam");
        let sys_root = root.join("sys");
        let dev_root = root.join("dev");
        fs::create_dir_all(sys_root.join("video10")).expect("create video10 sysfs node");
        fs::create_dir_all(sys_root.join("video2")).expect("create video2 sysfs node");
        fs::create_dir_all(sys_root.join("not-video")).expect("create ignored sysfs node");
        fs::create_dir_all(&dev_root).expect("create dev root");
        fs::write(dev_root.join("video10"), "").expect("create video10 device node");
        fs::write(dev_root.join("video2"), "").expect("create video2 device node");
        fs::write(sys_root.join("video10").join("name"), "USB Camera\n")
            .expect("write video10 name");
        fs::write(sys_root.join("video2").join("name"), "\n").expect("write video2 name");

        let devices = list_video_devices_from_roots(&sys_root, &dev_root);

        assert_eq!(
            devices,
            vec![
                VideoDevice {
                    path: dev_root.join("video2"),
                    node_name: "video2".into(),
                    display_name: "video2".into(),
                },
                VideoDevice {
                    path: dev_root.join("video10"),
                    node_name: "video10".into(),
                    display_name: "USB Camera".into(),
                },
            ]
        );
        fs::remove_dir_all(root).expect("remove webcam test dir");
    }

    #[test]
    fn list_video_devices_ignores_missing_dev_nodes() {
        let root = unique_test_dir("big-os-kit-webcam-missing-dev");
        let sys_root = root.join("sys");
        let dev_root = root.join("dev");
        fs::create_dir_all(sys_root.join("video0")).expect("create video0 sysfs node");
        fs::create_dir_all(&dev_root).expect("create dev root");

        let devices = list_video_devices_from_roots(&sys_root, &dev_root);

        assert!(devices.is_empty());
        fs::remove_dir_all(root).expect("remove webcam missing-dev test dir");
    }

    #[cfg(feature = "webcam-v4l2")]
    #[test]
    fn video_capture_device_from_v4l2_capabilities_rejects_non_capture_and_loopback_nodes() {
        let node = video_device("video4", "Integrated Camera");
        let non_capture = v4l2_capabilities("uvcvideo", "Integrated Camera", false);
        let loopback = v4l2_capabilities("v4l2loopback", "Virtual Camera", true);

        assert!(video_capture_device_from_v4l2_capabilities(node.clone(), &non_capture).is_none());
        assert!(video_capture_device_from_v4l2_capabilities(node, &loopback).is_none());
    }

    #[cfg(feature = "webcam-v4l2")]
    #[test]
    fn video_capture_device_from_v4l2_capabilities_uses_card_name_and_metadata() {
        let node = video_device("video8", "sysfs name");
        let capabilities = v4l2_capabilities("uvcvideo", "USB Camera", true);

        let device = video_capture_device_from_v4l2_capabilities(node, &capabilities)
            .expect("capture device");

        assert_eq!(device.path, PathBuf::from("/dev/video8"));
        assert_eq!(device.node_name, "video8");
        assert_eq!(device.display_name, "USB Camera");
        assert_eq!(device.driver, "uvcvideo");
        assert_eq!(device.bus_info, "usb-0000:00:14.0-6");
    }

    #[cfg(feature = "webcam-v4l2")]
    #[test]
    fn parse_v4l2_ctl_all_extracts_capture_metadata() {
        let output = r#"
Driver Info:
        Driver name      : uvcvideo
        Card type        : Integrated Camera
        Bus info         : usb-0000:00:14.0-6
        Device Caps      : 0x04200001
                Video Capture
                Streaming
"#;

        let capabilities = parse_v4l2_ctl_all(output).expect("capture capabilities");

        assert_eq!(capabilities.driver, "uvcvideo");
        assert_eq!(capabilities.card, "Integrated Camera");
        assert_eq!(capabilities.bus_identifier, "usb-0000:00:14.0-6");
        assert!(capabilities.has_video_capture);
    }

    #[cfg(feature = "webcam-v4l2")]
    fn video_device(node_name: &str, display_name: &str) -> VideoDevice {
        VideoDevice {
            path: PathBuf::from("/dev").join(node_name),
            node_name: node_name.to_string(),
            display_name: display_name.to_string(),
        }
    }

    #[cfg(feature = "webcam-v4l2")]
    fn v4l2_capabilities(
        driver: &str,
        card: &str,
        has_video_capture: bool,
    ) -> V4l2CaptureCapabilities {
        V4l2CaptureCapabilities {
            driver: driver.to_string(),
            card: card.to_string(),
            bus_identifier: "usb-0000:00:14.0-6".to_string(),
            has_video_capture,
        }
    }

    fn unique_test_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }
}
