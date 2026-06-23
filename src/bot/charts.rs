use std::fs;
use std::path::PathBuf;

use plotters::prelude::*;
use tracing::instrument;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::services::market_service::{MarketTimeSeries, MarketView};

const CHART_SIZE: (u32, u32) = (1400, 900);

pub struct ChartArtifact {
    pub filename: String,
    pub bytes: Vec<u8>,
}

#[instrument(skip(config, view))]
pub fn render_option_price_histogram(
    config: &AppConfig,
    view: &MarketView,
) -> AppResult<ChartArtifact> {
    let bars = view
        .detail
        .options
        .iter()
        .zip(view.probabilities.iter().copied())
        .map(|(option, probability)| {
            (
                option.label.clone(),
                probability,
                format!("{:.2}% • {} shares", probability * 100.0, option.shares_outstanding.round()),
            )
        })
        .collect::<Vec<_>>();

    render_bar_chart(
        config,
        "price-histogram",
        &format!("Option Price Histogram · #{}", view.detail.market.id),
        &view.detail.market.question,
        "Price per share",
        &bars,
        1.0,
    )
}

#[instrument(skip(config, holders))]
pub fn render_holder_concentration_histogram(
    config: &AppConfig,
    market_id: i64,
    question: &str,
    metric_label: &str,
    holders: &[(String, f64, String)],
) -> AppResult<ChartArtifact> {
    let bars = holders
        .iter()
        .map(|(name, value, detail)| (name.clone(), *value, detail.clone()))
        .collect::<Vec<_>>();

    render_bar_chart(
        config,
        "holder-histogram",
        &format!("Holder Concentration · #{}", market_id),
        &format!("{question} · Ranked by {metric_label}"),
        metric_label,
        &bars,
        bars.iter().map(|(_, value, _)| *value).fold(0.0, f64::max),
    )
}

#[instrument(skip(config, positions))]
pub fn render_position_histogram(
    config: &AppConfig,
    market_id: i64,
    question: &str,
    positions: &[(String, f64, String)],
) -> AppResult<ChartArtifact> {
    let bars = positions
        .iter()
        .map(|(label, shares, detail)| (label.clone(), *shares, detail.clone()))
        .collect::<Vec<_>>();

    render_bar_chart(
        config,
        "position-histogram",
        &format!("Your Exposure · #{}", market_id),
        question,
        "Shares held",
        &bars,
        bars.iter().map(|(_, value, _)| *value).fold(0.0, f64::max),
    )
}

#[instrument(skip(config, history))]
pub fn render_time_series_chart(
    config: &AppConfig,
    history: &MarketTimeSeries,
) -> AppResult<ChartArtifact> {
    let path = next_chart_path(config, "time-histogram")?;
    {
        let root = BitMapBackend::new(&path, CHART_SIZE).into_drawing_area();
        root.fill(&RGBColor(247, 243, 233))
            .map_err(plotters_err)?;
        let plot = root.margin(24, 24, 24, 24);
        let card = plot
            .titled(
                &format!("Price History · #{} · {}", history.market_id, history.question),
                ("sans-serif", 34).into_font().style(FontStyle::Bold),
            )
            .map_err(plotters_err)?;

        let series_points = history
            .series
            .iter()
            .flat_map(|series| series.points.iter().map(|point| point.at))
            .collect::<Vec<_>>();
        let Some(mut min_at) = series_points.iter().min().copied() else {
            return Err(AppError::Validation(
                "not enough market history exists to draw that chart yet".to_string(),
            ));
        };
        let Some(mut max_at) = series_points.iter().max().copied() else {
            return Err(AppError::Validation(
                "not enough market history exists to draw that chart yet".to_string(),
            ));
        };
        if min_at == max_at {
            max_at += chrono::Duration::minutes(1);
            min_at -= chrono::Duration::minutes(1);
        }

        let mut chart = ChartBuilder::on(&card)
            .margin(20)
            .x_label_area_size(60)
            .y_label_area_size(80)
            .build_cartesian_2d(min_at..max_at, 0f64..1.0)
            .map_err(plotters_err)?;

        chart
            .configure_mesh()
            .bold_line_style(RGBAColor(40, 60, 90, 0.12))
            .light_line_style(RGBAColor(40, 60, 90, 0.06))
            .axis_style(RGBColor(64, 74, 82))
            .y_desc("Probability / price per share")
            .x_desc("Snapshot time")
            .y_label_formatter(&|value| format!("{:.0}%", value * 100.0))
            .x_label_formatter(&|value| value.format("%m-%d %H:%M").to_string())
            .label_style(("sans-serif", 20).into_font().color(&RGBColor(64, 74, 82)))
            .draw()
            .map_err(plotters_err)?;

        for (index, series) in history.series.iter().enumerate() {
            let color = palette(index);
            chart
                .draw_series(LineSeries::new(
                    series.points.iter().map(|point| (point.at, point.probability)),
                    color.stroke_width(4),
                ))
                .map_err(plotters_err)?
                .label(short_label(&series.label, 18))
                .legend(move |(x, y)| {
                    PathElement::new(vec![(x, y), (x + 28, y)], color.stroke_width(4))
                });

            chart
                .draw_series(series.points.iter().map(|point| {
                    Circle::new((point.at, point.probability), 4, color.filled())
                }))
                .map_err(plotters_err)?;
        }

        chart
            .configure_series_labels()
            .background_style(RGBAColor(255, 255, 255, 0.92))
            .border_style(RGBColor(203, 213, 225))
            .label_font(("sans-serif", 20).into_font())
            .draw()
            .map_err(plotters_err)?;

        root.present().map_err(plotters_err)?;
    }
    read_chart_artifact(path)
}

fn render_bar_chart(
    config: &AppConfig,
    prefix: &str,
    title: &str,
    subtitle: &str,
    y_label: &str,
    bars: &[(String, f64, String)],
    suggested_max: f64,
) -> AppResult<ChartArtifact> {
    if bars.is_empty() {
        return Err(AppError::Validation(
            "not enough data exists to draw that chart yet".to_string(),
        ));
    }

    let path = next_chart_path(config, prefix)?;
    {
        let root = BitMapBackend::new(&path, CHART_SIZE).into_drawing_area();
        root.fill(&RGBColor(247, 243, 233))
            .map_err(plotters_err)?;
        let plot = root.margin(24, 24, 24, 24);

        let title_area = plot
            .titled(
                title,
                ("sans-serif", 34).into_font().style(FontStyle::Bold),
            )
            .map_err(plotters_err)?;
        title_area
            .draw(&Text::new(
                subtitle.to_string(),
                (32, 56),
                ("sans-serif", 22).into_font().color(&RGBColor(97, 107, 117)),
            ))
            .map_err(plotters_err)?;

        let labels = bars
            .iter()
            .map(|(label, _, _)| short_label(label, 18))
            .collect::<Vec<_>>();
        let upper = (suggested_max.max(0.01) * 1.20).max(1.0);
        let mut chart = ChartBuilder::on(&title_area)
            .margin(20)
            .x_label_area_size(90)
            .y_label_area_size(100)
            .build_cartesian_2d(0i32..bars.len() as i32, 0f64..upper)
            .map_err(plotters_err)?;

        chart
            .configure_mesh()
            .disable_x_mesh()
            .bold_line_style(RGBAColor(40, 60, 90, 0.10))
            .light_line_style(RGBAColor(40, 60, 90, 0.05))
            .axis_style(RGBColor(64, 74, 82))
            .x_labels(labels.len())
            .x_label_formatter(&|index| labels.get(*index as usize).cloned().unwrap_or_default())
            .y_desc(y_label)
            .label_style(("sans-serif", 20).into_font().color(&RGBColor(64, 74, 82)))
            .draw()
            .map_err(plotters_err)?;

        chart
            .draw_series(bars.iter().enumerate().map(|(index, (_, value, _))| {
                let x0 = index as i32;
                let x1 = x0 + 1;
                Rectangle::new([(x0, 0.0), (x1, *value)], palette(index).filled())
            }))
            .map_err(plotters_err)?;

        chart
            .draw_series(bars.iter().enumerate().map(|(index, (_, value, _))| {
                let label_y = (*value + upper * 0.03).min(upper * 0.98);
                Text::new(
                    format_value(*value),
                    (index as i32, label_y),
                    ("sans-serif", 20).into_font().style(FontStyle::Bold),
                )
            }))
            .map_err(plotters_err)?;

        chart
            .draw_series(bars.iter().enumerate().map(|(index, (_, value, detail))| {
                let label_y = (*value + upper * 0.07).min(upper * 0.98);
                Text::new(
                    short_label(detail, 28),
                    (index as i32, label_y),
                    ("sans-serif", 15).into_font().color(&RGBColor(97, 107, 117)),
                )
            }))
            .map_err(plotters_err)?;

        root.present().map_err(plotters_err)?;
    }
    read_chart_artifact(path)
}

fn next_chart_path(config: &AppConfig, prefix: &str) -> AppResult<PathBuf> {
    let chart_dir = config.cache_dir.join("charts");
    fs::create_dir_all(&chart_dir)?;
    let filename = format!("{prefix}-{}.png", Uuid::new_v4());
    Ok(chart_dir.join(filename))
}

fn read_chart_artifact(path: PathBuf) -> AppResult<ChartArtifact> {
    let bytes = fs::read(&path)?;
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppError::Other(anyhow::anyhow!("chart path was not valid UTF-8")))?;
    Ok(ChartArtifact {
        filename: filename.to_string(),
        bytes,
    })
}

fn plotters_err<E: std::fmt::Display>(error: E) -> AppError {
    AppError::Other(anyhow::anyhow!("chart rendering failed: {error}"))
}

fn short_label(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let head = trimmed.chars().take(max_chars.saturating_sub(1)).collect::<String>();
    format!("{head}…")
}

fn format_value(value: f64) -> String {
    if value >= 100.0 {
        format!("{value:.0}")
    } else if value >= 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    }
}

fn palette(index: usize) -> RGBColor {
    const COLORS: [RGBColor; 8] = [
        RGBColor(20, 184, 166),
        RGBColor(249, 115, 22),
        RGBColor(59, 130, 246),
        RGBColor(217, 70, 239),
        RGBColor(234, 179, 8),
        RGBColor(34, 197, 94),
        RGBColor(244, 63, 94),
        RGBColor(14, 165, 233),
    ];
    COLORS[index % COLORS.len()]
}
