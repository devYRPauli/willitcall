use std::collections::BTreeMap;
use std::fmt::Write as _;

use wic_core::result::{DecodeMode, Status};
use wic_core::ScenarioCategory;

use super::data::{SiteDataset, StackRow, CATEGORIES};
use super::html;
use super::svg::{self, Figure, FigureAccessibility};

const MARK_SIZE: u32 = 13;
const CELL_STEP: u32 = 15;
const ROW_STEP: u32 = 16;

pub(super) fn render_outcomes(dataset: &SiteDataset) -> String {
    let mut figures = String::from(
        "    <section class=\"analysis analysis-primary\" aria-labelledby=\"primary-analysis-title\">\n      <h2 id=\"primary-analysis-title\">Scenario evidence</h2>\n      <p>The complete raster shows all 50 scenarios; the focused raster isolates the seven multi-turn scenarios; the signature inventory groups only identical 50-outcome vectors. Figure captions link to the complete text alternative in JSON and CSV.</p>\n",
    );
    figures.push_str(&render_scenario_raster(dataset));
    figures.push_str(&render_multi_turn_raster(dataset));
    figures.push_str(&render_signature_inventory(dataset));
    figures.push_str("    </section>\n");
    figures
}

pub(super) fn render_pass_counts(dataset: &SiteDataset) -> String {
    let mut figures = String::from(
        "    <section class=\"analysis analysis-secondary\" aria-labelledby=\"analysis-title\">\n      <p class=\"eyebrow\">One-run observations</p>\n      <h2 id=\"analysis-title\">Observed pass counts</h2>\n      <p>Pass counts are shown in fixed stack order and withheld for rows with errors or skips. They are not scores or a ranking.</p>\n",
    );
    figures.push_str(&render_pass_count_strip(dataset));
    figures.push_str("    </section>\n");
    figures
}

#[derive(Clone)]
struct ScenarioColumn {
    id: String,
    category: ScenarioCategory,
}

struct PreparedMark {
    x: u32,
    y: u32,
    label: String,
    state: svg::MarkState,
}

struct PreparedText {
    x: u32,
    y: u32,
    text: String,
    class: &'static str,
    anchor: svg::TextAnchor,
    rotation: Option<i16>,
}

fn render_scenario_raster(dataset: &SiteDataset) -> String {
    let columns = scenario_columns(dataset);
    let row_indices = stable_row_indices(dataset);
    let left = 320;
    let top = 170;
    let width = left + columns.len() as u32 * CELL_STEP + 20;
    let height = top + row_indices.len() as u32 * ROW_STEP + 20;
    let (texts, rules) = raster_axes(&columns, &row_indices, dataset, left, top, height);
    let mut prepared = Vec::new();

    for (row_position, row_index) in row_indices.iter().copied().enumerate() {
        let row = &dataset.rows[row_index];
        for (column_position, column) in columns.iter().enumerate() {
            let status = scenario_status(row, &column.id);
            let Some(status) = status else {
                continue;
            };
            prepared.push(PreparedMark {
                x: left + column_position as u32 * CELL_STEP,
                y: top + row_position as u32 * ROW_STEP,
                label: column.id.clone(),
                state: mark_state(status),
            });
        }
    }
    let marks = borrow_marks(&prepared);
    let texts = borrow_texts(&texts);
    let caption = "Rows are published stack runs and columns are scenario ids grouped by capability. The n=1 hatch is intentionally omitted because every raster row is a single run, so hatching every cell would encode no difference.";
    let mut figure = html::render_figure(Figure::StatusRaster {
        accessibility: FigureAccessibility {
            number: 1,
            title: "Scenario-status raster",
            description: "Four-state outcomes for every published stack and scenario. Cell titles contain only the scenario id and outcome; full text is linked in JSON and CSV.",
            caption,
            does_not_show: "A leaderboard, an overall score, or how a stack would behave across repeated runs.",
        },
        width,
        height,
        marks: &marks,
        texts: &texts,
        rules: &rules,
    });
    let mobile = render_mobile_scenario_rasters(dataset, &columns, &row_indices);
    figure = figure.replacen("</figure>", &format!("{mobile}</figure>"), 1);
    figure
}

fn render_mobile_scenario_rasters(
    dataset: &SiteDataset,
    columns: &[ScenarioColumn],
    row_indices: &[usize],
) -> String {
    let mut html = String::from(
        "  <div class=\"mobile-raster\" aria-label=\"Scenario-status raster split into category panels\">\n",
    );
    for category in CATEGORIES {
        let category_columns = columns
            .iter()
            .filter(|column| column.category == category)
            .collect::<Vec<_>>();
        write!(
            html,
            "    <section class=\"mini-raster-panel\" data-category=\"{}\"><h3>{}</h3><div class=\"mini-raster-grid\" style=\"grid-template-columns:8.5rem repeat({},0.78rem)\">\n      <span class=\"mini-raster-corner\">Stack</span>",
            category,
            category_label(category),
            category_columns.len(),
        )
        .expect("write mobile raster");
        for column in &category_columns {
            write!(
                html,
                "<span class=\"mini-column-label\">{}</span>",
                html::escape_html(&column.id),
            )
            .expect("write mobile raster column");
        }
        html.push('\n');
        for row_index in row_indices {
            let row = &dataset.rows[*row_index];
            let stack = stack_label(row);
            write!(
                html,
                "      <span class=\"mini-row-label\">{}</span>",
                html::escape_html(&stack),
            )
            .expect("write mobile raster row label");
            for column in &category_columns {
                if let Some(status) = scenario_status(row, &column.id) {
                    let label = format!("{}: {}", column.id, status_label(status));
                    write!(
                        html,
                        "<i class=\"mini-mark {}\" role=\"img\" aria-label=\"{}\" title=\"{}\"></i>",
                        status_label(status),
                        html::escape_html(&label),
                        html::escape_html(&label),
                    )
                    .expect("write mobile raster mark");
                } else {
                    html.push_str("<i class=\"mini-mark missing\" aria-hidden=\"true\"></i>");
                }
            }
            html.push('\n');
        }
        html.push_str("    </div></section>\n");
    }
    html.push_str("  </div>\n");
    html
}

fn render_multi_turn_raster(dataset: &SiteDataset) -> String {
    let columns = scenario_columns(dataset)
        .into_iter()
        .filter(|column| column.category == ScenarioCategory::MultiTurn)
        .collect::<Vec<_>>();
    let row_indices = stable_row_indices(dataset);
    let left = 320;
    let top = 170;
    let width = left + columns.len() as u32 * CELL_STEP + 20;
    let height = top + row_indices.len() as u32 * ROW_STEP + 20;
    let (texts, rules) = raster_axes(&columns, &row_indices, dataset, left, top, height);
    let mut prepared = Vec::new();
    let mut passed = 0;
    let mut total = 0;
    let mut zero_pass_rows = 0;

    for (row_position, row_index) in row_indices.iter().copied().enumerate() {
        let row = &dataset.rows[row_index];
        let row_passes = row
            .scenarios
            .iter()
            .filter(|scenario| {
                scenario.category == ScenarioCategory::MultiTurn && scenario.status == Status::Pass
            })
            .count();
        if row_passes == 0 {
            zero_pass_rows += 1;
        }
        for (column_position, column) in columns.iter().enumerate() {
            let status = scenario_status(row, &column.id);
            if let Some(status) = status {
                total += 1;
                if status == Status::Pass {
                    passed += 1;
                }
                prepared.push(PreparedMark {
                    x: left + column_position as u32 * CELL_STEP,
                    y: top + row_position as u32 * ROW_STEP,
                    label: column.id.clone(),
                    state: mark_state(status),
                });
            }
        }
    }
    let percentage = if total == 0 {
        0
    } else {
        (passed * 100 + total / 2) / total
    };
    let caption = format!(
        "Across these published observations, multi_turn passes {passed}/{total} ({percentage}%), and {zero_pass_rows} of {} rows pass none of the multi_turn scenarios.",
        row_indices.len()
    );
    let marks = borrow_marks(&prepared);
    let texts = borrow_texts(&texts);
    html::render_figure(Figure::StatusRaster {
        accessibility: FigureAccessibility {
            number: 2,
            title: "Multi-turn raster",
            description: "Four-state outcomes for the multi_turn scenarios across every published stack run.",
            caption: &caption,
            does_not_show: "Why a turn failed, an isolated model or server effect, or repeated-run reliability.",
        },
        width,
        height,
        marks: &marks,
        texts: &texts,
        rules: &rules,
    })
}

fn render_signature_inventory(dataset: &SiteDataset) -> String {
    let columns = scenario_columns(dataset);
    let row_indices = stable_row_indices(dataset);
    let mut grouped = BTreeMap::<String, Vec<usize>>::new();
    for row_index in row_indices {
        let row = &dataset.rows[row_index];
        grouped
            .entry(signature_key(row, &columns))
            .or_default()
            .push(row_index);
    }

    let left = 100;
    let names_x = left + columns.len() as u32 * CELL_STEP + 18;
    let top = 170;
    let mut y = top;
    let mut prepared = Vec::new();
    let mut owned_texts = Vec::new();
    let mut repeated_seven_signature = None;

    for (signature_index, members) in grouped.values().enumerate() {
        let signature_name = format!("Signature {:02}", signature_index + 1);
        let member_labels = members
            .iter()
            .map(|index| stack_label(&dataset.rows[*index]))
            .collect::<Vec<_>>();
        let row_height = (member_labels.len() as u32 * 14 + 4).max(20);
        owned_texts.push(PreparedText {
            x: left - 8,
            y: y + 11,
            text: signature_name.clone(),
            class: "raster-row-label",
            anchor: svg::TextAnchor::End,
            rotation: None,
        });
        for (member_index, label) in member_labels.iter().enumerate() {
            owned_texts.push(PreparedText {
                x: names_x,
                y: y + 11 + member_index as u32 * 14,
                text: label.clone(),
                class: "signature-stack-label",
                anchor: svg::TextAnchor::Start,
                rotation: None,
            });
        }

        let representative = &dataset.rows[members[0]];
        for (column_index, column) in columns.iter().enumerate() {
            if let Some(status) = scenario_status(representative, &column.id) {
                prepared.push(PreparedMark {
                    x: left + column_index as u32 * CELL_STEP,
                    y,
                    label: column.id.clone(),
                    state: mark_state(status),
                });
            }
        }

        let statuses = columns
            .iter()
            .filter_map(|column| {
                scenario_status(representative, &column.id).map(|status| (column.category, status))
            })
            .collect::<Vec<_>>();
        let passes = statuses
            .iter()
            .filter(|(_, status)| *status == Status::Pass)
            .count();
        let negative_trap_passes = statuses
            .iter()
            .filter(|(category, status)| {
                *category == ScenarioCategory::NegativeTrap && *status == Status::Pass
            })
            .count();
        let tool_choice_passes = statuses
            .iter()
            .filter(|(category, status)| {
                *category == ScenarioCategory::ToolChoiceModes && *status == Status::Pass
            })
            .count();
        let includes_granite = members.iter().any(|index| {
            dataset.rows[*index]
                .display_name
                .to_ascii_lowercase()
                .contains("granite")
        });
        if passes == 7 && members.len() > 1 && includes_granite {
            repeated_seven_signature =
                Some((members.len(), negative_trap_passes, tool_choice_passes));
        }
        y += row_height;
    }

    let width = names_x + 620;
    let height = y + 15;
    let (axis_texts, rules) = column_axes(&columns, left, top, height);
    owned_texts.extend(axis_texts);
    let caption = repeated_seven_signature.map_or_else(
        || "Each row is one distinct exact status vector; the names at right are the stack observations sharing it.".to_owned(),
        |(count, negative_trap_passes, tool_choice_passes)| format!("Each row is one distinct exact status vector. The repeated 7-pass signature is shared by {count} stacks, including the granite observations; it contains the same {negative_trap_passes} negative_trap and {tool_choice_passes} tool_choice_modes passes in every row."),
    );
    let marks = borrow_marks(&prepared);
    let texts = borrow_texts(&owned_texts);
    html::render_figure(Figure::StatusRaster {
        accessibility: FigureAccessibility {
            number: 3,
            title: "Outcome-signature inventory",
            description: "Distinct exact scenario-status vectors and the published stack observations sharing each vector.",
            caption: &caption,
            does_not_show: "Shared model identity, a common causal mechanism, or expected future behavior.",
        },
        width,
        height,
        marks: &marks,
        texts: &texts,
        rules: &rules,
    })
}

fn render_pass_count_strip(dataset: &SiteDataset) -> String {
    let row_indices = stable_row_indices(dataset);
    let (fully_measurable, not_fully_measurable): (Vec<_>, Vec<_>) =
        row_indices.into_iter().partition(|index| {
            let counts = &dataset.rows[*index].category_counts;
            counts.errors == 0 && counts.skipped == 0
        });
    let max_count = dataset
        .rows
        .iter()
        .map(|row| row.category_counts.passed + row.category_counts.failed)
        .max()
        .unwrap_or(1)
        .max(1);
    let plot_left = 80;
    let plot_width = 800;
    let axis_y = 48;
    let mut pass_occurrences = BTreeMap::<usize, usize>::new();
    let mut prepared_dots = Vec::new();

    for row_index in fully_measurable.iter().copied() {
        let row = &dataset.rows[row_index];
        let passed = row.category_counts.passed;
        let occurrence = pass_occurrences.entry(passed).or_default();
        let y = axis_y + 20 + *occurrence as u32 * 13;
        *occurrence += 1;
        prepared_dots.push((
            plot_left + passed as u32 * plot_width / max_count as u32,
            y,
            passed,
            format!("{}: {passed} passes", stack_label(row)),
        ));
    }

    let max_overlap = pass_occurrences.values().copied().max().unwrap_or(1) as u32;
    let not_panel_y = axis_y + 35 + max_overlap * 13;
    let mut owned_texts = vec![PreparedText {
        x: plot_left,
        y: 20,
        text: "Fully measurable observations".to_owned(),
        class: "plot-panel-label",
        anchor: svg::TextAnchor::Start,
        rotation: None,
    }];
    let mut rules = vec![svg::FigureRule {
        x1: plot_left,
        y1: axis_y,
        x2: plot_left + plot_width,
        y2: axis_y,
        class: "strip-axis",
    }];
    let step = (max_count / 5).max(1);
    let mut tick = 0;
    while tick <= max_count {
        add_strip_tick(
            &mut owned_texts,
            &mut rules,
            plot_left,
            plot_width,
            axis_y,
            max_count,
            tick,
        );
        tick += step;
    }
    if (max_count / step) * step != max_count {
        add_strip_tick(
            &mut owned_texts,
            &mut rules,
            plot_left,
            plot_width,
            axis_y,
            max_count,
            max_count,
        );
    }
    owned_texts.push(PreparedText {
        x: plot_left,
        y: not_panel_y,
        text: "Not fully measurable - pass counts withheld".to_owned(),
        class: "plot-panel-label",
        anchor: svg::TextAnchor::Start,
        rotation: None,
    });
    rules.push(svg::FigureRule {
        x1: plot_left,
        y1: not_panel_y + 8,
        x2: plot_left + plot_width,
        y2: not_panel_y + 8,
        class: "panel-rule",
    });

    let mut prepared_not_measurable = Vec::new();
    for (position, row_index) in not_fully_measurable.iter().copied().enumerate() {
        let row = &dataset.rows[row_index];
        let y = not_panel_y + 22 + position as u32 * 20;
        let detail = format!(
            "{} errors, {} skipped; pass count not plotted",
            row.category_counts.errors, row.category_counts.skipped
        );
        prepared_not_measurable.push((plot_left, y, stack_label(row), detail.clone()));
        owned_texts.push(PreparedText {
            x: plot_left + 20,
            y: y + 11,
            text: format!("{} - {detail}", stack_label(row)),
            class: "not-measurable-label",
            anchor: svg::TextAnchor::Start,
            rotation: None,
        });
    }

    let dots = prepared_dots
        .iter()
        .map(|(x, y, pass_count, label)| svg::StripDot {
            x: *x,
            y: *y,
            pass_count: *pass_count,
            label,
        })
        .collect::<Vec<_>>();
    let not_measurable_marks = prepared_not_measurable
        .iter()
        .map(|(x, y, label, detail)| svg::NotFullyMeasurableMark {
            x: *x,
            y: *y,
            label,
            detail,
        })
        .collect::<Vec<_>>();
    let texts = borrow_texts(&owned_texts);
    let height = not_panel_y + 35 + not_fully_measurable.len() as u32 * 20;
    html::render_figure(Figure::StripPlot {
        accessibility: FigureAccessibility {
            number: 4,
            title: "Observed pass-count strip plot",
            description: "One pass-count dot per fully measurable published stack observation, with error-bearing or skipped rows listed separately and not assigned a pass count.",
            caption: "Distribution of published one-run stack observations. Pass counts are plotted only when every scenario has a pass or fail verdict; rows with errors or skips are listed in the separate panel.",
            does_not_show: "A model score distribution, rankings, uncertainty, or repeated-run variability.",
        },
        width: plot_left + plot_width + 40,
        height,
        dots: &dots,
        not_fully_measurable: &not_measurable_marks,
        texts: &texts,
        rules: &rules,
    })
}

fn raster_axes(
    columns: &[ScenarioColumn],
    row_indices: &[usize],
    dataset: &SiteDataset,
    left: u32,
    top: u32,
    height: u32,
) -> (Vec<PreparedText>, Vec<svg::FigureRule>) {
    let (mut texts, rules) = column_axes(columns, left, top, height);
    for (row_position, row_index) in row_indices.iter().copied().enumerate() {
        texts.push(PreparedText {
            x: left - 8,
            y: top + row_position as u32 * ROW_STEP + 11,
            text: stack_label(&dataset.rows[row_index]),
            class: "raster-row-label",
            anchor: svg::TextAnchor::End,
            rotation: None,
        });
    }
    (texts, rules)
}

fn column_axes(
    columns: &[ScenarioColumn],
    left: u32,
    top: u32,
    height: u32,
) -> (Vec<PreparedText>, Vec<svg::FigureRule>) {
    let mut texts = Vec::new();
    let mut rules = Vec::new();
    for (column_position, column) in columns.iter().enumerate() {
        let x = left + column_position as u32 * CELL_STEP + MARK_SIZE / 2;
        texts.push(PreparedText {
            x,
            y: top - 12,
            text: column.id.clone(),
            class: "raster-column-label",
            anchor: svg::TextAnchor::Start,
            rotation: Some(-60),
        });
    }
    for category in CATEGORIES {
        let positions = columns
            .iter()
            .enumerate()
            .filter(|(_, column)| column.category == category)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let (Some(first), Some(last)) = (positions.first(), positions.last()) else {
            continue;
        };
        let start = left + *first as u32 * CELL_STEP;
        let end = left + (*last as u32 + 1) * CELL_STEP;
        texts.push(PreparedText {
            x: start + (end - start) / 2,
            y: 16,
            text: category.to_string(),
            class: "raster-group-label",
            anchor: svg::TextAnchor::Middle,
            rotation: None,
        });
        rules.push(svg::FigureRule {
            x1: start,
            y1: 24,
            x2: start,
            y2: height - 10,
            class: "category-rule",
        });
    }
    (texts, rules)
}

fn add_strip_tick(
    texts: &mut Vec<PreparedText>,
    rules: &mut Vec<svg::FigureRule>,
    plot_left: u32,
    plot_width: u32,
    axis_y: u32,
    max_count: usize,
    value: usize,
) {
    let x = plot_left + value as u32 * plot_width / max_count as u32;
    texts.push(PreparedText {
        x,
        y: axis_y - 7,
        text: value.to_string(),
        class: "strip-tick-label",
        anchor: svg::TextAnchor::Middle,
        rotation: None,
    });
    rules.push(svg::FigureRule {
        x1: x,
        y1: axis_y - 4,
        x2: x,
        y2: axis_y + 5,
        class: "strip-tick",
    });
}

fn scenario_columns(dataset: &SiteDataset) -> Vec<ScenarioColumn> {
    let mut columns = Vec::new();
    for category in CATEGORIES {
        let mut category_columns = BTreeMap::<String, ScenarioColumn>::new();
        for scenario in dataset
            .rows
            .iter()
            .flat_map(|row| row.scenarios.iter())
            .filter(|scenario| scenario.category == category)
        {
            category_columns
                .entry(scenario.id.clone())
                .or_insert_with(|| ScenarioColumn {
                    id: scenario.id.clone(),
                    category,
                });
        }
        columns.extend(category_columns.into_values());
    }
    columns
}

fn stable_row_indices(dataset: &SiteDataset) -> Vec<usize> {
    let mut indices = (0..dataset.rows.len()).collect::<Vec<_>>();
    indices.sort_by_key(|index| stable_stack_key(&dataset.rows[*index]));
    indices
}

fn stable_stack_key(row: &StackRow) -> String {
    let model = &row.metadata.model;
    let artifact = &model.artifact;
    let quantization = artifact
        .quantization
        .as_ref()
        .map(|quantization| quantization.label.as_str())
        .unwrap_or("");
    format!(
        "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
        model.canonical_id.as_deref().unwrap_or(""),
        row.display_name,
        decode_order(row.decode_mode),
        row.metadata.server.preset_name,
        artifact.source_id.as_deref().unwrap_or(""),
        artifact.sha256.as_deref().unwrap_or(""),
        quantization,
        row.metadata
            .server
            .reported_version
            .as_deref()
            .unwrap_or(""),
        row.endpoint_display,
        row.file_name,
    )
}

fn decode_order(mode: DecodeMode) -> u8 {
    match mode {
        DecodeMode::GrammarConstrained => 0,
        DecodeMode::UnconstrainedPostHoc => 1,
        DecodeMode::Unknown => 2,
    }
}

fn category_label(category: ScenarioCategory) -> &'static str {
    match category {
        ScenarioCategory::SingleCall => "Single call",
        ScenarioCategory::ToolChoiceModes => "Tool choice",
        ScenarioCategory::NegativeTrap => "Correctly declines",
        ScenarioCategory::MultiTurn => "Multi-turn",
        ScenarioCategory::ParallelCalls => "Parallel calls",
        ScenarioCategory::Streaming => "Streaming",
    }
}

fn stack_label(row: &StackRow) -> String {
    let quantization = row
        .metadata
        .model
        .artifact
        .quantization
        .as_ref()
        .map(|quantization| quantization.label.as_str())
        .unwrap_or("quant not declared");
    format!(
        "{} | {} | {}",
        row.display_name,
        quantization,
        display_server(&row.metadata.server.preset_name)
    )
}

fn display_server(server: &str) -> &str {
    match server {
        "llamacpp" => "llama.cpp",
        "mlx_lm" => "MLX LM",
        other => other,
    }
}

fn scenario_status(row: &StackRow, scenario_id: &str) -> Option<Status> {
    row.scenarios
        .iter()
        .find(|scenario| scenario.id == scenario_id)
        .map(|scenario| scenario.status)
}

fn signature_key(row: &StackRow, columns: &[ScenarioColumn]) -> String {
    columns
        .iter()
        .map(|column| match scenario_status(row, &column.id) {
            Some(Status::Pass) => 'P',
            Some(Status::Fail) => 'F',
            Some(Status::Error) => 'E',
            Some(Status::Skipped) => 'S',
            None => '-',
        })
        .collect()
}

fn borrow_marks(prepared: &[PreparedMark]) -> Vec<svg::StatusMark<'_>> {
    prepared
        .iter()
        .map(|mark| svg::StatusMark {
            x: mark.x,
            y: mark.y,
            label: &mark.label,
            state: mark.state,
        })
        .collect()
}

fn borrow_texts(prepared: &[PreparedText]) -> Vec<svg::FigureText<'_>> {
    prepared
        .iter()
        .map(|text| svg::FigureText {
            x: text.x,
            y: text.y,
            text: &text.text,
            class: text.class,
            anchor: text.anchor,
            rotation: text.rotation,
        })
        .collect()
}

fn mark_state(status: Status) -> svg::MarkState {
    match status {
        Status::Pass => svg::MarkState::Pass,
        Status::Fail => svg::MarkState::Fail,
        Status::Error => svg::MarkState::Error,
        Status::Skipped => svg::MarkState::Skipped,
    }
}

fn status_label(status: Status) -> &'static str {
    match status {
        Status::Pass => "pass",
        Status::Fail => "fail",
        Status::Error => "error",
        Status::Skipped => "skipped",
    }
}
