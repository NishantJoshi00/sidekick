//! Pure aggregation: `Vec<Event>` → `Stats`.
//!
//! The aggregate is renderer-agnostic. Every renderer should be able to draw
//! its view from this struct alone — no re-reading events, no extra queries.

use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, Duration, NaiveDate, Timelike, Utc};

use crate::analytics::event::{Decision, DecisionReason, Event, ToolKind};

#[derive(Debug, Clone, Copy)]
pub enum TimeRange {
    Week,
    Month,
    Year,
    All,
}

impl TimeRange {
    pub fn label(&self) -> &'static str {
        match self {
            TimeRange::Week => "this week",
            TimeRange::Month => "this month",
            TimeRange::Year => "this year",
            TimeRange::All => "all time",
        }
    }

    pub fn cutoff(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self {
            TimeRange::Week => Some(now - Duration::days(7)),
            TimeRange::Month => Some(now - Duration::days(30)),
            TimeRange::Year => Some(now - Duration::days(365)),
            TimeRange::All => None,
        }
    }
}

/// Aggregated stats — the input every renderer reads from.
///
/// New fields can be added freely; renderers only see what they consume.
/// `#[allow(dead_code)]` reserves a small amount of surface for future
/// renderer iterations (per-file sparklines, reason breakdowns) without
/// thrashing the schema each time we add a chart.
#[derive(Debug, Clone)]
pub struct Stats {
    pub range: TimeRange,
    pub generated_at: DateTime<Utc>,

    /// Hero: total mutations the AI tried that landed on a dirty current buffer.
    pub saves: u32,
    /// Total hook decisions in window (allowed + denied).
    pub total_decisions: u32,
    /// Decisions that resulted in allow. Reserved for renderer iteration.
    #[allow(dead_code)]
    pub allowed: u32,
    /// nvim launches in window. Reserved for renderer iteration.
    #[allow(dead_code)]
    pub nvim_launches: u32,
    /// Buffers refreshed post-write. Reserved for renderer iteration.
    #[allow(dead_code)]
    pub refreshes: u32,
    /// Distinct files touched (decided on or refreshed). Reserved.
    #[allow(dead_code)]
    pub unique_files: u32,

    // ── The Mirror ───────────────────────────────────────────────────────
    /// Per-bucket "you" activity: nvim launches + buffer-dirty-current
    /// incidents. The top sparkline.
    pub you_buckets: Vec<u32>,
    /// Per-bucket AI activity: every hook decision. The bottom sparkline.
    pub ai_buckets: Vec<u32>,
    /// Per-bucket save count (decisions blocked because buffer was dirty
    /// and current). The dot trail between top and bottom.
    pub save_buckets: Vec<u32>,
    /// For axis labels — at which bucket index does each day start?
    pub day_markers: Vec<(usize, NaiveDate)>,

    // ── Meta (variable reward) ────────────────────────────────────────────
    /// `sidekick stats` invocations today (date-local match).
    pub views_today: u32,
    /// `sidekick stats` invocations within the active window.
    #[allow(dead_code)]
    pub views_in_window: u32,
    /// `sidekick stats` invocations across all time.
    pub views_total: u32,
    /// Hours since the previous `sidekick stats` invocation (None if first).
    pub hours_since_last_view: Option<i64>,

    /// Calendar of activity, keyed by local-date, value = total mutation events.
    pub by_day: BTreeMap<NaiveDate, DayActivity>,
    /// 24-bucket hour-of-day distribution of mutation events.
    pub by_hour: [u32; 24],
    /// Top files (flat) — sorted by total mutation events (desc).
    pub top_files: Vec<FileStats>,
    /// Projects with their files nested inside, sorted by total (desc).
    /// This is the 2D view: which project, and within it, which files.
    /// Reserved for renderer iteration.
    #[allow(dead_code)]
    pub projects: Vec<ProjectStats>,
    /// Tool breakdown. Reserved for renderer iteration.
    #[allow(dead_code)]
    pub by_tool: BTreeMap<&'static str, u32>,
    /// Reason breakdown. Reserved for renderer iteration.
    #[allow(dead_code)]
    pub by_reason: BTreeMap<&'static str, u32>,
}

#[derive(Debug, Clone, Default)]
pub struct DayActivity {
    pub decisions: u32,
    pub saves: u32,
    pub refreshes: u32,
    pub launches: u32,
    /// Hour-of-day buckets for this day. Drives the per-day sparkline.
    pub hours: [u32; 24],
}

#[derive(Debug, Clone)]
pub struct FileStats {
    pub path: String,
    pub total: u32,
    pub saves: u32,
    /// Sparkline buckets across the window. Reserved for renderer iteration.
    #[allow(dead_code)]
    pub sparkline: Vec<u32>,
    /// Per-bucket save count for this file. Drives per-file pipe traces.
    pub save_buckets: Vec<u32>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProjectStats {
    pub name: String,
    pub cwd: String,
    pub total: u32,
    pub saves: u32,
    pub files: Vec<FileStats>,
}

pub fn aggregate(events: Vec<Event>, range: TimeRange) -> Stats {
    let now = Utc::now();
    let cutoff = range.cutoff(now);

    let filtered: Vec<&Event> = events
        .iter()
        .filter(|e| cutoff.is_none_or(|c| e.timestamp() >= c))
        .collect();

    let window_start: Option<DateTime<Utc>> =
        cutoff.or_else(|| filtered.first().map(|e| e.timestamp()));

    let mut saves = 0u32;
    let mut total_decisions = 0u32;
    let mut allowed = 0u32;
    let mut nvim_launches = 0u32;
    let mut refreshes = 0u32;
    let mut by_day: BTreeMap<NaiveDate, DayActivity> = BTreeMap::new();
    let mut by_hour = [0u32; 24];
    let mut by_tool: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut by_reason: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut file_totals: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    let mut file_timeline: BTreeMap<String, Vec<DateTime<Utc>>> = BTreeMap::new();
    let mut file_save_timeline: BTreeMap<String, Vec<DateTime<Utc>>> = BTreeMap::new();
    // (cwd) → (file) → (total, saves)
    let mut project_files: BTreeMap<String, BTreeMap<String, (u32, u32)>> = BTreeMap::new();
    let mut unique: HashSet<String> = HashSet::new();

    for e in &filtered {
        let ts = e.timestamp();
        let date = ts.date_naive();
        let hour = ts.hour() as usize;

        match e {
            Event::HookDecision(d) => {
                total_decisions += 1;
                by_hour[hour] += 1;
                let day = by_day.entry(date).or_default();
                day.decisions += 1;
                day.hours[hour] += 1;

                *by_tool.entry(tool_label(d.tool)).or_default() += 1;
                *by_reason.entry(reason_label(d.reason)).or_default() += 1;

                let (total, save_count) = file_totals.entry(d.file.clone()).or_default();
                *total += 1;
                unique.insert(d.file.clone());
                file_timeline.entry(d.file.clone()).or_default().push(ts);

                let (pf_total, pf_saves) = project_files
                    .entry(d.cwd.clone())
                    .or_default()
                    .entry(d.file.clone())
                    .or_default();
                *pf_total += 1;

                match d.decision {
                    Decision::Allow => allowed += 1,
                    Decision::Deny => {
                        if matches!(d.reason, DecisionReason::BufferDirtyAndCurrent) {
                            saves += 1;
                            day.saves += 1;
                            *save_count += 1;
                            *pf_saves += 1;
                            file_save_timeline
                                .entry(d.file.clone())
                                .or_default()
                                .push(ts);
                        }
                    }
                }
            }
            Event::BufferRefresh(r) => {
                refreshes += 1;
                by_day.entry(date).or_default().refreshes += 1;
                unique.insert(r.file.clone());
            }
            Event::NvimLaunch(_) => {
                nvim_launches += 1;
                by_day.entry(date).or_default().launches += 1;
            }
            Event::StatsView(_) => {
                // View events don't count toward the data story; they feed
                // meta-observations computed below.
            }
        }
    }

    // Same bucket count as the Mirror grid below so per-file save buckets
    // align with the global timeline and the renderer can use them directly.
    let bucket_count = 49usize;
    let span_start = window_start.unwrap_or(now);
    let span_secs = (now - span_start).num_seconds().max(1) as f64;

    let mut top_files: Vec<FileStats> = file_totals
        .into_iter()
        .map(|(path, (total, save_count))| {
            let timeline = file_timeline.remove(&path).unwrap_or_default();
            let mut buckets = vec![0u32; bucket_count];
            for ts in timeline {
                let offset = (ts - span_start).num_seconds().max(0) as f64;
                let idx = ((offset / span_secs) * bucket_count as f64) as usize;
                let idx = idx.min(bucket_count - 1);
                buckets[idx] += 1;
            }
            // Per-file save buckets — used by the renderer to draw one pipe
            // per top file. Each file's saves form its own winding path.
            let save_timeline = file_save_timeline.remove(&path).unwrap_or_default();
            let mut save_bkts = vec![0u32; bucket_count];
            for ts in save_timeline {
                let offset = (ts - span_start).num_seconds().max(0) as f64;
                let idx = ((offset / span_secs) * bucket_count as f64) as usize;
                let idx = idx.min(bucket_count - 1);
                save_bkts[idx] += 1;
            }
            FileStats {
                path,
                total,
                saves: save_count,
                sparkline: buckets,
                save_buckets: save_bkts,
            }
        })
        .collect();
    top_files.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.path.cmp(&b.path)));

    // Lookup so project-nested files can inherit the same time-bucket sparkline
    // computed for the global ranking — avoids a second pass through events.
    let file_sparklines: std::collections::HashMap<String, Vec<u32>> = top_files
        .iter()
        .map(|f| (f.path.clone(), f.sparkline.clone()))
        .collect();
    let file_save_bkts: std::collections::HashMap<String, Vec<u32>> = top_files
        .iter()
        .map(|f| (f.path.clone(), f.save_buckets.clone()))
        .collect();

    let mut projects: Vec<ProjectStats> = project_files
        .into_iter()
        .map(|(cwd, files_map)| {
            let mut files: Vec<FileStats> = files_map
                .into_iter()
                .map(|(path, (total, saves))| {
                    let sparkline = file_sparklines.get(&path).cloned().unwrap_or_default();
                    let save_buckets = file_save_bkts.get(&path).cloned().unwrap_or_default();
                    FileStats {
                        path,
                        total,
                        saves,
                        sparkline,
                        save_buckets,
                    }
                })
                .collect();
            files.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.path.cmp(&b.path)));
            let total = files.iter().map(|f| f.total).sum();
            let saves_count = files.iter().map(|f| f.saves).sum();
            let name = project_name(&cwd).to_string();
            ProjectStats {
                name,
                cwd,
                total,
                saves: saves_count,
                files,
            }
        })
        .collect();
    projects.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.name.cmp(&b.name)));

    // ── The Mirror: bucket events across the window ──────────────────────
    const BUCKET_COUNT: usize = 49;
    let window_start_ts = window_start.unwrap_or(now);
    let span_secs = (now - window_start_ts).num_seconds().max(1) as f64;

    let bucket_idx = |ts: DateTime<Utc>| -> usize {
        let offset = (ts - window_start_ts).num_seconds().max(0) as f64;
        let idx = ((offset / span_secs) * BUCKET_COUNT as f64) as usize;
        idx.min(BUCKET_COUNT - 1)
    };

    let mut you_buckets = vec![0u32; BUCKET_COUNT];
    let mut ai_buckets = vec![0u32; BUCKET_COUNT];
    let mut save_buckets = vec![0u32; BUCKET_COUNT];

    for e in &filtered {
        let idx = bucket_idx(e.timestamp());
        match e {
            Event::NvimLaunch(_) => {
                you_buckets[idx] += 1;
            }
            Event::HookDecision(d) => {
                ai_buckets[idx] += 1;
                if matches!(d.decision, Decision::Deny)
                    && matches!(d.reason, DecisionReason::BufferDirtyAndCurrent)
                {
                    save_buckets[idx] += 1;
                    // Note: `you_buckets` deliberately does NOT include saves.
                    // Letting saves bump both lines makes them look parallel and
                    // kills the visual tension. We want "you" to read as your
                    // *arrival moments* (nvim launches), distinct from the AI's
                    // constant hum.
                }
            }
            Event::BufferRefresh(_) | Event::StatsView(_) => {}
        }
    }

    // Day markers: the first bucket of each day in the window.
    let mut day_markers: Vec<(usize, NaiveDate)> = Vec::new();
    let total_days = (now - window_start_ts).num_days().max(0) + 1;
    let mut seen_dates: std::collections::BTreeSet<NaiveDate> = std::collections::BTreeSet::new();
    for day_offset in 0..total_days {
        let day_ts = window_start_ts + Duration::days(day_offset);
        let date = day_ts.date_naive();
        if seen_dates.insert(date) {
            let idx = bucket_idx(day_ts);
            day_markers.push((idx, date));
        }
    }

    // ── Meta: stats-view counts ──────────────────────────────────────────
    let today = now.date_naive();
    let mut views_today = 0u32;
    let mut views_in_window = 0u32;
    let mut views_total = 0u32;
    let mut last_view_ts: Option<DateTime<Utc>> = None;
    for e in &events {
        if let Event::StatsView(v) = e {
            views_total += 1;
            if cutoff.is_none_or(|c| v.at >= c) {
                views_in_window += 1;
            }
            if v.at.date_naive() == today {
                views_today += 1;
            }
            // The "previous" view, not counting the one we just appended for
            // this invocation. We compute by skipping ts that match `now`
            // within a small tolerance.
            let delta = now - v.at;
            if delta.num_seconds() > 5 && last_view_ts.is_none_or(|t| v.at > t) {
                last_view_ts = Some(v.at);
            }
        }
    }
    let hours_since_last_view = last_view_ts.map(|t| (now - t).num_hours());

    Stats {
        range,
        generated_at: now,
        saves,
        total_decisions,
        allowed,
        nvim_launches,
        refreshes,
        unique_files: unique.len() as u32,
        by_day,
        by_hour,
        top_files,
        projects,
        by_tool,
        by_reason,
        you_buckets,
        ai_buckets,
        save_buckets,
        day_markers,
        views_today,
        views_in_window,
        views_total,
        hours_since_last_view,
    }
}

/// Extract the project name (last non-empty path component) from a cwd.
fn project_name(cwd: &str) -> &str {
    cwd.rsplit('/').find(|s| !s.is_empty()).unwrap_or(cwd)
}

fn tool_label(t: ToolKind) -> &'static str {
    match t {
        ToolKind::Edit => "Edit",
        ToolKind::Write => "Write",
        ToolKind::MultiEdit => "MultiEdit",
    }
}

fn reason_label(r: DecisionReason) -> &'static str {
    match r {
        DecisionReason::NoNvimRunning => "no_nvim_running",
        DecisionReason::StatusCheckFailed => "status_check_failed",
        DecisionReason::BufferDirtyAndCurrent => "buffer_dirty_and_current",
        DecisionReason::BufferAvailable => "buffer_available",
    }
}
