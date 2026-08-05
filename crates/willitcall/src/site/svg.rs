use std::fmt::Write as _;

const DENSE_MARK_SIZE: u32 = 13;
const AGGREGATE_MARK_SIZE: u32 = 44;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MarkState {
    Pass,
    Fail,
    Error,
    Skipped,
}

impl MarkState {
    fn label(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Error => "error",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct StatusMark<'a> {
    pub x: u32,
    pub y: u32,
    pub label: &'a str,
    pub state: MarkState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AggregateMark<'a> {
    pub x: u32,
    pub y: u32,
    pub label: &'a str,
    pub state: MarkState,
    pub passed: u32,
    pub measured: u32,
    pub no_verdict: bool,
}

/// Renders prepared 13 px status marks. Positions and statuses are supplied by the caller.
pub(super) fn render_status_marks(
    title: &str,
    description: &str,
    width: u32,
    height: u32,
    marks: &[StatusMark<'_>],
) -> String {
    let mut body = String::new();
    let mut rows = Vec::with_capacity(marks.len());

    for mark in marks {
        writeln!(
            body,
            "    <g class=\"status-mark\" data-state=\"{}\" aria-label=\"{}: {}\">",
            mark.state.label(),
            escape_html(mark.label),
            mark.state.label()
        )
        .expect("write SVG");
        write_mark_shape(&mut body, mark.state, mark.x, mark.y, DENSE_MARK_SIZE);
        body.push_str("    </g>\n");
        rows.push(vec![mark.label.to_owned(), mark.state.label().to_owned()]);
    }

    render_accessible_figure(
        title,
        description,
        width,
        height,
        &body,
        &["Item", "Status"],
        &rows,
    )
}

/// Renders prepared 44 px aggregate cells. The caller explicitly selects `no_verdict` per cell.
pub(super) fn render_aggregate_marks(
    title: &str,
    description: &str,
    width: u32,
    height: u32,
    marks: &[AggregateMark<'_>],
) -> String {
    let mut body = String::new();
    let mut rows = Vec::with_capacity(marks.len());

    for mark in marks {
        writeln!(
            body,
            "    <g class=\"aggregate-mark\" data-state=\"{}\" aria-label=\"{}: {} of {} passed, {}\">",
            mark.state.label(),
            escape_html(mark.label),
            mark.passed,
            mark.measured,
            mark.state.label()
        )
        .expect("write SVG");
        write_mark_shape(&mut body, mark.state, mark.x, mark.y, AGGREGATE_MARK_SIZE);
        let text_ink = match mark.state {
            MarkState::Pass | MarkState::Fail => "var(--design-paper, #fcfbf9)",
            MarkState::Error | MarkState::Skipped => "var(--design-ink, #1c1c1c)",
        };
        writeln!(
            body,
            "      <text class=\"aggregate-value\" x=\"{}\" y=\"{}\" text-anchor=\"middle\" fill=\"{text_ink}\">{}/{}</text>",
            mark.x + AGGREGATE_MARK_SIZE / 2,
            mark.y + AGGREGATE_MARK_SIZE / 2 + 3,
            mark.passed,
            mark.measured
        )
        .expect("write SVG");
        if mark.no_verdict {
            body.push_str(&render_no_verdict_overlay(mark.x, mark.y));
        }
        body.push_str("    </g>\n");
        rows.push(vec![
            mark.label.to_owned(),
            mark.state.label().to_owned(),
            format!("{}/{}", mark.passed, mark.measured),
            if mark.no_verdict {
                "n=1, no verdict".to_owned()
            } else {
                String::new()
            },
        ]);
    }

    render_accessible_figure(
        title,
        description,
        width,
        height,
        &body,
        &["Item", "Status", "Passed / measured", "Verdict"],
        &rows,
    )
}

/// Returns only the aggregate-cell overlay so dense raster renderers cannot apply it implicitly.
pub(super) fn render_no_verdict_overlay(x: u32, y: u32) -> String {
    let mut overlay =
        String::from("      <g class=\"no-verdict-overlay\" aria-label=\"n=1, no verdict\">\n");
    let end = AGGREGATE_MARK_SIZE;
    for offset in (6..end).step_by(7) {
        writeln!(
            overlay,
            "        <path class=\"no-verdict-hatch\" d=\"M {} {} L {} {} M {} {} L {} {}\" fill=\"none\" stroke=\"var(--design-ink, #1c1c1c)\" stroke-width=\"0.75\" opacity=\"0.18\" />",
            x + offset,
            y,
            x + end,
            y + end - offset,
            x,
            y + offset,
            x + end - offset,
            y + end
        )
        .expect("write SVG");
    }
    write!(
        overlay,
        "        <rect class=\"no-verdict-border\" x=\"{}\" y=\"{}\" width=\"{end}\" height=\"{end}\" fill=\"none\" stroke=\"var(--design-ink, #1c1c1c)\" stroke-width=\"0.75\" stroke-dasharray=\"3 2\" />\n        <text class=\"no-verdict-label\" x=\"{}\" y=\"{}\" text-anchor=\"middle\" fill=\"var(--design-ink, #1c1c1c)\" stroke=\"var(--design-paper, #fcfbf9)\" stroke-width=\"2\" paint-order=\"stroke\"><tspan baseline-shift=\"super\">n=1</tspan></text>\n      </g>\n",
        x,
        y,
        x + end / 2,
        y + end - 5
    )
    .expect("write SVG");
    overlay
}

fn write_mark_shape(svg: &mut String, state: MarkState, x: u32, y: u32, size: u32) {
    let inset = 1;
    let mark_size = size - inset * 2;
    let start = inset + 1;
    let end = size - inset - 1;
    match state {
        MarkState::Pass => {
            writeln!(
                svg,
                "      <rect class=\"state-shape state-pass\" x=\"{}\" y=\"{}\" width=\"{mark_size}\" height=\"{mark_size}\" fill=\"var(--design-pass, #00513a)\" stroke=\"none\" />",
                x + inset,
                y + inset
            )
            .expect("write SVG");
        }
        MarkState::Fail => {
            write!(
                svg,
                "      <rect class=\"state-shape state-fail\" x=\"{}\" y=\"{}\" width=\"{mark_size}\" height=\"{mark_size}\" fill=\"var(--design-fail, #d55e00)\" stroke=\"none\" />\n      <line class=\"state-slash knockout-slash\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"var(--design-paper, #fcfbf9)\" stroke-width=\"1.5\" stroke-linecap=\"square\" />\n",
                x + inset,
                y + inset,
                x + start,
                y + start,
                x + end,
                y + end
            )
            .expect("write SVG");
        }
        MarkState::Error => {
            write!(
                svg,
                "      <rect class=\"state-shape state-error\" x=\"{}\" y=\"{}\" width=\"{mark_size}\" height=\"{mark_size}\" fill=\"none\" stroke=\"var(--design-ink, #1c1c1c)\" stroke-width=\"0.75\" />\n      <line class=\"state-slash ink-slash\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"var(--design-ink, #1c1c1c)\" stroke-width=\"1\" />\n",
                x + inset,
                y + inset,
                x + start,
                y + start,
                x + end,
                y + end
            )
            .expect("write SVG");
        }
        MarkState::Skipped => {
            writeln!(
                svg,
                "      <rect class=\"state-shape state-skipped\" x=\"{}\" y=\"{}\" width=\"{mark_size}\" height=\"{mark_size}\" fill=\"none\" stroke=\"var(--design-grey-2, #8c8a85)\" stroke-width=\"0.6\" stroke-dasharray=\"1 1.75\" opacity=\"0.45\" />",
                x + inset,
                y + inset
            )
            .expect("write SVG");
        }
    }
}

fn render_accessible_figure(
    title: &str,
    description: &str,
    width: u32,
    height: u32,
    body: &str,
    headers: &[&str],
    rows: &[Vec<String>],
) -> String {
    let mut figure = String::new();
    write!(
        figure,
        "<figure class=\"svg-figure\">\n  <svg role=\"img\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n    <title>{}</title>\n    <desc>{}</desc>\n{body}  </svg>\n  <table class=\"svg-text-fallback\">\n    <caption>{}</caption>\n    <thead><tr>",
        escape_html(title),
        escape_html(description),
        escape_html(title)
    )
    .expect("write SVG figure");
    for header in headers {
        write!(figure, "<th scope=\"col\">{}</th>", escape_html(header))
            .expect("write SVG fallback");
    }
    figure.push_str("</tr></thead>\n    <tbody>\n");
    for row in rows {
        figure.push_str("      <tr>");
        for cell in row {
            write!(figure, "<td>{}</td>", escape_html(cell)).expect("write SVG fallback");
        }
        figure.push_str("</tr>\n");
    }
    figure.push_str("    </tbody>\n  </table>\n</figure>\n");
    figure
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            character if character.is_ascii() => escaped.push(character),
            character => write!(escaped, "&#{};", character as u32).expect("write entity"),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_one_status(state: MarkState, label: &str) -> String {
        render_status_marks(
            "Status figure",
            "One prepared status mark.",
            DENSE_MARK_SIZE,
            DENSE_MARK_SIZE,
            &[StatusMark {
                x: 0,
                y: 0,
                label,
                state,
            }],
        )
    }

    fn aggregate(no_verdict: bool) -> String {
        render_aggregate_marks(
            "Aggregate figure",
            "One prepared aggregate mark.",
            AGGREGATE_MARK_SIZE,
            AGGREGATE_MARK_SIZE,
            &[AggregateMark {
                x: 0,
                y: 0,
                label: "single call",
                state: MarkState::Pass,
                passed: 1,
                measured: 1,
                no_verdict,
            }],
        )
    }

    #[test]
    fn four_states_have_distinct_non_colour_shapes() {
        let pass = render_one_status(MarkState::Pass, "pass result");
        assert!(pass.contains("class=\"state-shape state-pass\""));
        assert!(pass.contains("stroke=\"none\""));
        assert!(!pass.contains("state-slash"));

        let fail = render_one_status(MarkState::Fail, "fail result");
        assert!(fail.contains("class=\"state-shape state-fail\""));
        assert!(fail.contains("class=\"state-slash knockout-slash\""));
        assert!(fail.contains("stroke-width=\"1.5\""));

        let error = render_one_status(MarkState::Error, "error result");
        assert!(error.contains("class=\"state-shape state-error\""));
        assert!(error.contains("class=\"state-slash ink-slash\""));
        assert!(error.contains("fill=\"none\""));

        let skipped = render_one_status(MarkState::Skipped, "skipped result");
        assert!(skipped.contains("class=\"state-shape state-skipped\""));
        assert!(skipped.contains("stroke-dasharray=\"1 1.75\""));
        assert!(skipped.contains("opacity=\"0.45\""));
        assert!(!skipped.contains("state-slash"));
    }

    #[test]
    fn every_renderer_includes_title_description_and_table_fallback() {
        for figure in [
            render_one_status(MarkState::Pass, "result"),
            aggregate(false),
        ] {
            assert!(figure.contains("<title>"));
            assert!(figure.contains("</title>"));
            assert!(figure.contains("<desc>"));
            assert!(figure.contains("</desc>"));
            assert!(figure.contains("<table class=\"svg-text-fallback\">"));
        }
    }

    #[test]
    fn interpolated_labels_are_escaped_everywhere() {
        let figure = render_one_status(MarkState::Pass, "model <x> & \"quoted\"");
        assert!(figure.contains("model &lt;x&gt; &amp; &quot;quoted&quot;"));
        assert!(!figure.contains("model <x>"));
        assert!(!figure.contains("\"quoted\""));
    }

    #[test]
    fn no_verdict_overlay_is_opt_in_and_aggregate_only() {
        let with_overlay = aggregate(true);
        assert!(with_overlay.contains("class=\"no-verdict-overlay\""));
        assert!(with_overlay.contains("class=\"no-verdict-hatch\""));
        assert!(with_overlay.contains("stroke-dasharray=\"3 2\""));
        assert!(with_overlay.contains("baseline-shift=\"super\">n=1"));

        assert!(!aggregate(false).contains("no-verdict-overlay"));
        assert!(!render_one_status(MarkState::Pass, "result").contains("no-verdict-overlay"));
    }

    #[test]
    fn output_contains_no_external_url() {
        let figures = [
            render_one_status(MarkState::Fail, "result"),
            aggregate(true),
        ];
        for figure in figures {
            assert!(!figure.contains("http://"));
            assert!(!figure.contains("https://"));
        }
    }

    #[test]
    fn stylesheet_defines_light_and_dark_instrument_tokens() {
        let style = super::super::STYLE;
        for declaration in [
            "--design-paper: #fcfbf9",
            "--design-ink: #1c1c1c",
            "--design-grey-1: #4f4e4b",
            "--design-grey-2: #8c8a85",
            "--design-grey-3: #d5d2cc",
            "--design-pass: #00513a",
            "--design-fail: #d55e00",
            "--design-link-accent: #0f6d6d",
            "--type-prose: 16px",
            "--type-table: 13px",
            "--type-raster-label: 11px",
            "--type-heading-1: 20px",
            "--type-heading-2: 17px",
            "--type-heading-weight: 600",
            "--measure-prose: 46rem",
            "--measure-figure: 76rem",
            "@media (prefers-color-scheme: dark)",
            "--design-paper: #131417",
            "--design-ink: #e8e6e1",
        ] {
            assert!(style.contains(declaration), "missing {declaration}");
        }
    }
}
