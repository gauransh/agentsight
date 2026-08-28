use crate::{RepositoryTraceOptions, build_repository_trace};
use base64::{Engine, engine::general_purpose::STANDARD};
use headless_chrome::{Browser, LaunchOptions, protocol::cdp::Emulation};
use serde_json::json;
use std::collections::HashSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

const WIDTH: u32 = 1264;
const HEIGHT: u32 = 936;
const COMPACT_FPS: f64 = 30.0;
const FULL_FPS: f64 = 8.0;
const RUNTIME: &str = include_str!("../vendor/vis/repository-nebula.iife.js");

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompactRate {
    DurationSeconds(f64),
    Full,
}

impl FromStr for CompactRate {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let value = raw.trim().to_ascii_lowercase();
        if value == "full" {
            return Ok(Self::Full);
        }
        let (number, multiplier) = value
            .strip_suffix('s')
            .map(|number| (number, 1.0))
            .or_else(|| value.strip_suffix('m').map(|number| (number, 60.0)))
            .or_else(|| value.strip_suffix('h').map(|number| (number, 3_600.0)))
            .unwrap_or((&value, 1.0));
        let seconds = number
            .parse::<f64>()
            .map_err(|_| "compact rate must be a duration such as 30s, 2m, or full".to_string())?
            * multiplier;
        if !seconds.is_finite() || seconds < 1.0 {
            return Err("compact rate duration must be at least one second".into());
        }
        Ok(Self::DurationSeconds(seconds))
    }
}

#[derive(Debug)]
struct MediaPlan {
    indices: Vec<usize>,
    frame_rate: f64,
    duration_seconds: f64,
    compact: bool,
}

fn media_plan(action_count: usize, rate: CompactRate) -> MediaPlan {
    let count = action_count.max(1);
    match rate {
        CompactRate::Full => MediaPlan {
            indices: (0..count).collect(),
            frame_rate: FULL_FPS,
            duration_seconds: count as f64 / FULL_FPS,
            compact: false,
        },
        CompactRate::DurationSeconds(seconds) => {
            let frame_count =
                count.min((seconds * COMPACT_FPS).round().max(count.min(2) as f64) as usize);
            let indices = if frame_count == 1 {
                vec![0]
            } else {
                (0..frame_count)
                    .map(|frame| frame * (count - 1) / (frame_count - 1))
                    .collect()
            };
            MediaPlan {
                indices,
                frame_rate: frame_count as f64 / seconds,
                duration_seconds: seconds,
                compact: frame_count < count,
            }
        }
    }
}

pub fn run_vis(
    repo: &Path,
    outputs: &[PathBuf],
    global: bool,
    compact_rate: CompactRate,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let started = Instant::now();
    let outputs = requested_outputs(outputs)?;
    eprintln!("[evolution 1/5] repository  {}", repo.display());
    eprintln!(
        "[evolution 2/5] sessions    scanning Claude + Codex + Gemini + Cursor{}",
        if global { " globally" } else { "" }
    );
    let scan = Instant::now();
    let trace = build_repository_trace(&RepositoryTraceOptions {
        repo: repo.into(),
        global,
    })?;
    eprintln!(
        "[evolution 3/5] actions     {} sessions · {} source events · {} tool actions · {} file actions · {:.1}s",
        trace.session_count,
        trace.source_event_count,
        trace.events.len(),
        trace.file_action_count,
        scan.elapsed().as_secs_f32()
    );
    let action_count = trace.events.len();
    let payload = json!({
        "meta": {
            "repository": trace.repository,
            "endpoint_revision": trace.revision,
            "window_start_ms": trace.start_ms,
            "window_end_ms": trace.end_ms,
            "session_scope": if trace.global { "global_tool_operations" } else { "repository_identity" },
        },
        "events": trace.events,
        "commits": trace.commits_ms.into_iter().map(|committed_at_ms| json!({ "committed_at_ms": committed_at_ms })).collect::<Vec<_>>(),
    });
    let html = html_document(&payload)?;
    for output in &outputs {
        if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
    }
    for output in outputs.iter().filter(|path| extension(path) == "html") {
        fs::write(output, &html)?;
    }
    let media = outputs
        .iter()
        .filter(|path| extension(path) != "html")
        .cloned()
        .collect::<Vec<_>>();
    if !media.is_empty() {
        eprintln!("[evolution 4/5] render      preparing deterministic layout and media");
        render_media(&html, &media, media_plan(action_count, compact_rate))?;
    }
    for output in &outputs {
        eprintln!(
            "[evolution 5/5] output      {} · {} KiB",
            output.display(),
            fs::metadata(output)?.len().div_ceil(1024)
        );
    }
    eprintln!(
        "[evolution] complete    {:.1}s",
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

fn requested_outputs(outputs: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut seen = HashSet::new();
    let outputs = outputs
        .iter()
        .filter(|path| seen.insert((*path).clone()))
        .cloned()
        .collect::<Vec<_>>();
    if outputs.is_empty() {
        return Err("at least one --output is required".into());
    }
    for output in &outputs {
        if !matches!(
            extension(output).as_str(),
            "html" | "svg" | "png" | "gif" | "mp4"
        ) {
            return Err(format!(
                "unsupported output {}; use .html, .svg, .png, .gif, or .mp4",
                output.display()
            ));
        }
    }
    Ok(outputs)
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn render_media(
    html: &str,
    outputs: &[PathBuf],
    plan: MediaPlan,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let temporary = tempfile::tempdir()?;
    let page = temporary.path().join("repository-nebula.html");
    fs::write(&page, html)?;
    let renderer = BrowserRenderer::open(&page)?;
    if outputs
        .iter()
        .any(|path| matches!(extension(path).as_str(), "gif" | "mp4"))
    {
        eprintln!(
            "[evolution] compact     {} media frames · {:.2} fps · {:.1}s{}",
            plan.indices.len(),
            plan.frame_rate,
            plan.duration_seconds,
            if plan.compact {
                " · uniform by action"
            } else {
                " · full"
            }
        );
        let mp4 = renderer.mp4(&plan)?;
        let mp4_path = temporary.path().join("repository-nebula.mp4");
        fs::write(&mp4_path, &mp4)?;
        write_copies(&mp4, outputs, "mp4")?;
        if outputs.iter().any(|path| extension(path) == "gif") {
            eprintln!("[evolution] gif         building bounded-memory palette");
            let palette_path = temporary.path().join("repository-nebula-palette.png");
            let palette = Command::new("ffmpeg")
                .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
                .arg(&mp4_path)
                .args(["-vf", "palettegen=max_colors=128", "-frames:v", "1"])
                .arg(&palette_path)
                .status()
                .map_err(|error| {
                    std::io::Error::new(
                        error.kind(),
                        format!("GIF export needs FFmpeg (`ffmpeg`): {error}"),
                    )
                })?;
            if !palette.success() {
                return Err("ffmpeg GIF palette generation failed".into());
            }
            eprintln!("[evolution] gif         converting shared MP4");
            let gif_path = temporary.path().join("repository-nebula.gif");
            let status = Command::new("ffmpeg")
                .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
                .arg(&mp4_path)
                .arg("-i")
                .arg(&palette_path)
                .args([
                    "-lavfi",
                    "[0:v][1:v]paletteuse=dither=sierra2_4a",
                    "-loop",
                    "0",
                ])
                .arg(&gif_path)
                .status()?;
            if !status.success() {
                return Err("ffmpeg GIF conversion failed".into());
            }
            write_copies(&fs::read(gif_path)?, outputs, "gif")?;
        }
    }
    if outputs.iter().any(|path| extension(path) == "png") {
        write_copies(
            &renderer.bytes("AgentVis.pngBase64()", false)?,
            outputs,
            "png",
        )?;
    }
    if outputs.iter().any(|path| extension(path) == "svg") {
        let svg = renderer.string("AgentVis.svg()", true)?;
        write_copies(
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{svg}").as_bytes(),
            outputs,
            "svg",
        )?;
    }
    Ok(())
}

fn write_copies(bytes: &[u8], outputs: &[PathBuf], format: &str) -> std::io::Result<()> {
    for output in outputs.iter().filter(|path| extension(path) == format) {
        fs::write(output, bytes)?;
    }
    Ok(())
}

struct BrowserRenderer {
    tab: Arc<headless_chrome::Tab>,
    _browser: Browser,
    _profile: tempfile::TempDir,
}

impl BrowserRenderer {
    fn open(page: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let profile = tempfile::tempdir()?;
        let options = LaunchOptions::default_builder()
            .path(Some(browser_binary()?))
            .user_data_dir(Some(profile.path().into()))
            .headless(true)
            .sandbox(false)
            .window_size(Some((WIDTH, HEIGHT)))
            .idle_browser_timeout(Duration::from_secs(600))
            .args(vec![
                OsStr::new("--hide-scrollbars"),
                OsStr::new("--force-device-scale-factor=1"),
            ])
            .build()?;
        let browser = Browser::new(options)?;
        let tab = browser.new_tab()?;
        tab.call_method(Emulation::SetDeviceMetricsOverride {
            width: WIDTH,
            height: HEIGHT,
            device_scale_factor: 1.0,
            mobile: false,
            scale: None,
            screen_width: Some(WIDTH),
            screen_height: Some(HEIGHT),
            position_x: None,
            position_y: None,
            dont_set_visible_size: None,
            screen_orientation: None,
            viewport: None,
            display_feature: None,
            device_posture: None,
        })?;
        tab.navigate_to(&format!("file://{}?still=1", page.display()))?
            .wait_until_navigated()?;
        let started = Instant::now();
        while !tab
            .evaluate("window.__AGENTVIS_READY__ === true", false)
            .ok()
            .and_then(|object| object.value)
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            if started.elapsed() > Duration::from_secs(120) {
                return Err("visual renderer did not become ready".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(Self {
            tab,
            _browser: browser,
            _profile: profile,
        })
    }

    fn string(
        &self,
        script: &str,
        await_promise: bool,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self.tab
            .evaluate(script, await_promise)?
            .value
            .and_then(|value| value.as_str().map(str::to_string))
            .ok_or_else(|| format!("renderer returned no string for {script}").into())
    }

    fn bytes(
        &self,
        script: &str,
        await_promise: bool,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(STANDARD.decode(self.string(script, await_promise)?)?)
    }

    fn mp4(&self, plan: &MediaPlan) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let media = serde_json::to_string(&json!({
            "indices": plan.indices,
            "frameRate": plan.frame_rate,
        }))?;
        let total = self
            .tab
            .evaluate(&format!("AgentVis.beginMp4({media})"), true)?
            .value
            .and_then(|value| value.as_u64())
            .ok_or("MP4 encoder returned no frame count")?;
        eprintln!("[evolution] frames      0/{total}");
        let mut done = 0;
        while done < total {
            let progress = serde_json::from_str::<Vec<u64>>(
                &self.string("AgentVis.encodeMp4Chunk(48).then(JSON.stringify)", true)?,
            )?;
            done = progress.first().copied().unwrap_or(done);
            eprintln!("[evolution] frames      {done}/{total}");
        }
        self.bytes("AgentVis.finishMp4()", true)
    }
}

fn browser_binary() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("CHROME")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Ok(path);
    }
    if let Some(home) = dirs::home_dir() {
        let cache = home.join(".cache/ms-playwright");
        if let Some(path) = browser_from_playwright_cache(&cache) {
            return Ok(path);
        }
    }
    for path in system_browser_candidates() {
        if path.is_file() {
            return Ok(path);
        }
    }
    for name in [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
    ] {
        if Command::new(name)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            return Ok(name.into());
        }
    }
    Err(
        "PNG/SVG/GIF/MP4 export needs Chrome, Edge, or Chromium; HTML has no browser dependency"
            .into(),
    )
}

fn browser_from_playwright_cache(cache: &Path) -> Option<PathBuf> {
    let mut versions = fs::read_dir(cache)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    versions.sort();
    for version in versions.into_iter().rev() {
        for suffix in [
            "chrome-linux64/chrome",
            "chrome-linux/chrome",
            "chrome-headless-shell-linux64/headless_shell",
            "chrome-win64/chrome.exe",
            "chrome-win/chrome.exe",
            "chrome-headless-shell-win64/headless_shell.exe",
        ] {
            let path = version.join(suffix);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn system_browser_candidates() -> Vec<PathBuf> {
    ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(env::var_os)
        .flat_map(|root| {
            let root = PathBuf::from(root);
            [
                root.join("Google/Chrome/Application/chrome.exe"),
                root.join("Microsoft/Edge/Application/msedge.exe"),
                root.join("Chromium/Application/chrome.exe"),
            ]
        })
        .collect()
}

fn html_document(payload: &serde_json::Value) -> Result<String, serde_json::Error> {
    let payload = serde_json::to_string(payload)?
        .replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    // Theme is stamped on <html> BEFORE the stylesheet applies: ?theme=light|dark
    // wins (an embedding product passes its shell's mode), the OS preference is
    // the fallback, dark is the artifact's native look. The runtime JS resolves
    // the same order for its canvas palette, so one reload flips everything.
    Ok(format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Agent Session Evolution Graph</title><script>(function(){{var p=new URLSearchParams(location.search).get("theme");var t=p==="light"||p==="dark"?p:(window.matchMedia&&matchMedia("(prefers-color-scheme: light)").matches?"light":"dark");document.documentElement.dataset.theme=t;}})()</script><style>
:root{{color-scheme:dark;--bg:#070b12;--ink:#dce8f7;--ink-dim:#71839a;--ink-mode:#8c9bb0;--ink-cursor:#9bacc0;--ink-footer:#7c8ba0;--hair:rgba(135,160,190,.18);--accent:#61d7bf;--accent-border:rgba(97,215,191,.4);--accent-bg:#10231f;--flash:#efd265;--flash-glow:rgba(239,210,101,.42);--sw-read:#f7ffff;--sw-write:#ff9678;--sw-create:#75f0a9;--sw-rename:#63dfff;--sw-delete:#ff647c}}
:root[data-theme=light]{{color-scheme:light;--bg:#f7f9fc;--ink:#1c2b3a;--ink-dim:#5c7086;--ink-mode:#5c7086;--ink-cursor:#44586e;--ink-footer:#71849a;--hair:rgba(45,65,90,.22);--accent:#0f7f6d;--accent-border:rgba(15,127,109,.4);--accent-bg:#e2f2ee;--flash:#a87c10;--flash-glow:rgba(168,124,16,.28);--sw-read:#1c2b3a;--sw-write:#d95c33;--sw-create:#0f8a4d;--sw-rename:#0f7fa8;--sw-delete:#c22945}}
:root{{font-family:Inter,ui-sans-serif,system-ui,sans-serif;background:var(--bg);color:var(--ink)}}*{{box-sizing:border-box}}body{{margin:0;background:var(--bg)}}.artifact{{width:1264px;min-height:865px;padding:24px 32px;background:var(--bg);transition:box-shadow .12s}}.artifact.commit-flash{{box-shadow:inset 0 0 0 2px var(--flash),0 0 30px var(--flash-glow)}}.header{{display:flex;justify-content:space-between;gap:24px;border-bottom:1px solid var(--hair);padding-bottom:14px}}.eyebrow,.mode{{font:11px ui-monospace,monospace;color:var(--accent);letter-spacing:.12em;text-transform:uppercase}}.header h1{{font-size:28px;margin:6px 0}}.header p{{font-size:12px;color:var(--ink-dim);margin:0}}.mode{{color:var(--ink-mode);border:1px solid var(--hair);padding:7px 9px;border-radius:99px;align-self:flex-start}}.visual{{width:1200px;height:675px;margin-top:14px}}.timeline{{display:grid;grid-template-columns:42px 1fr 190px;gap:12px;align-items:center;border-top:1px solid var(--hair);padding:14px 0 8px}}.timeline button{{width:36px;height:36px;border-radius:50%;border:1px solid var(--accent-border);background:var(--accent-bg);color:var(--accent)}}.timeline input{{width:100%;accent-color:var(--accent)}}.timeline output{{font:10px ui-monospace,monospace;color:var(--ink-cursor);text-align:right}}.legend{{display:flex;gap:18px;font:10px ui-monospace,monospace;color:var(--ink-dim)}}.legend i{{display:inline-block;width:13px;height:3px;margin-right:5px;vertical-align:middle}}.footer{{margin-top:8px;color:var(--ink-footer);font:10px ui-monospace,monospace}}
</style></head><body><main id="artifact" class="artifact"><header class="header"><div><span class="eyebrow">repository evolution</span><h1 id="view-title"></h1><p id="view-note"></p></div><span class="mode">Agent event time</span></header><section class="visual"><div id="chart"></div></section><section class="timeline"><button id="play">▶</button><input id="timeline" type="range"><output id="cursor-label"></output></section><section class="legend"><span><i style="background:var(--sw-read)"></i>read attention</span><span><i style="background:var(--sw-write)"></i>write ripple</span><span><i style="background:var(--sw-create)"></i>create</span><span><i style="background:var(--sw-rename)"></i>rename</span><span><i style="background:var(--sw-delete)"></i>delete</span><span><i style="background:var(--flash)"></i>commit frame</span></section><footer id="provenance" class="footer"></footer></main><script>{RUNTIME}</script><script>AgentVis.initialize({payload})</script></body></html>"#
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_formats_are_inferred_and_deduplicated() {
        let outputs =
            requested_outputs(&["x.html".into(), "x.gif".into(), "x.html".into()]).unwrap();
        assert_eq!(outputs.len(), 2);
        assert!(requested_outputs(&["x.pdf".into()]).is_err());
    }

    #[test]
    fn compact_rate_accepts_durations_and_full() {
        assert_eq!("30s".parse(), Ok(CompactRate::DurationSeconds(30.0)));
        assert_eq!("2m".parse(), Ok(CompactRate::DurationSeconds(120.0)));
        assert_eq!("full".parse(), Ok(CompactRate::Full));
        assert!("0s".parse::<CompactRate>().is_err());
    }

    #[test]
    fn playwright_cache_accepts_windows_chrome_layout() {
        let temp = tempfile::tempdir().unwrap();
        let browser = temp.path().join("chromium-1200/chrome-win64/chrome.exe");
        fs::create_dir_all(browser.parent().unwrap()).unwrap();
        fs::write(&browser, b"fixture").unwrap();

        assert_eq!(browser_from_playwright_cache(temp.path()), Some(browser));
    }

    #[test]
    fn compact_media_frames_are_uniform_by_action() {
        let plan = media_plan(68_222, CompactRate::DurationSeconds(30.0));
        assert_eq!(plan.indices.len(), 900);
        assert_eq!(plan.indices.first(), Some(&0));
        assert_eq!(plan.indices.last(), Some(&68_221));
        let gaps = plan
            .indices
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .collect::<Vec<_>>();
        assert!(gaps.iter().max().unwrap() - gaps.iter().min().unwrap() <= 1);
        assert_eq!(media_plan(337, CompactRate::Full).indices.len(), 337);
    }

    #[test]
    fn html_is_self_contained_and_script_safe() {
        let html = html_document(&json!({ "value": "</script>" })).unwrap();
        assert!(html.contains("<title>Agent Session Evolution Graph</title>"));
        assert!(html.contains("AgentVis.initialize"));
        assert!(!html.contains("\"</script>\""));
    }
}
