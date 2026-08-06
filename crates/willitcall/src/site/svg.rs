use std::fmt::Write as _;

const DENSE_MARK_SIZE: u32 = 13;
const AGGREGATE_MARK_SIZE: u32 = 44;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FigureAccessibility<'a> {
    pub number: u8,
    pub title: &'a str,
    pub description: &'a str,
    pub caption: &'a str,
    pub does_not_show: &'a str,
}

pub(super) struct TableFallback<'a> {
    pub headers: &'a [&'a str],
    pub rows: &'a [Vec<String>],
}

/// A closed set of figure primitives. Each variant requires the same accessibility
/// contract, and the module exposes no alternate complete-SVG renderer.
#[allow(dead_code)] // Brief 8 constructs the first production figure.
pub(super) enum Figure<'a> {
    StatusMarks {
        accessibility: FigureAccessibility<'a>,
        width: u32,
        height: u32,
        marks: &'a [StatusMark<'a>],
    },
    AggregateMarks {
        accessibility: FigureAccessibility<'a>,
        width: u32,
        height: u32,
        marks: &'a [AggregateMark<'a>],
    },
    StatusRaster {
        accessibility: FigureAccessibility<'a>,
        width: u32,
        height: u32,
        marks: &'a [StatusMark<'a>],
        texts: &'a [FigureText<'a>],
        rules: &'a [FigureRule],
        fallback: TableFallback<'a>,
    },
    Bars {
        accessibility: FigureAccessibility<'a>,
        width: u32,
        height: u32,
        bars: &'a [Bar<'a>],
        texts: &'a [FigureText<'a>],
        rules: &'a [FigureRule],
        fallback: TableFallback<'a>,
    },
    StripPlot {
        accessibility: FigureAccessibility<'a>,
        width: u32,
        height: u32,
        dots: &'a [StripDot<'a>],
        not_fully_measurable: &'a [NotFullyMeasurableMark<'a>],
        texts: &'a [FigureText<'a>],
        rules: &'a [FigureRule],
        fallback: TableFallback<'a>,
    },
}

pub(super) fn render_figure(figure: Figure<'_>) -> String {
    match figure {
        Figure::StatusMarks {
            accessibility,
            width,
            height,
            marks,
        } => render_status_marks(accessibility, width, height, marks),
        Figure::AggregateMarks {
            accessibility,
            width,
            height,
            marks,
        } => render_aggregate_marks(accessibility, width, height, marks),
        Figure::StatusRaster {
            accessibility,
            width,
            height,
            marks,
            texts,
            rules,
            fallback,
        } => render_status_raster(accessibility, width, height, marks, texts, rules, fallback),
        Figure::Bars {
            accessibility,
            width,
            height,
            bars,
            texts,
            rules,
            fallback,
        } => render_bars(accessibility, width, height, bars, texts, rules, fallback),
        Figure::StripPlot {
            accessibility,
            width,
            height,
            dots,
            not_fully_measurable,
            texts,
            rules,
            fallback,
        } => render_strip_plot(
            accessibility,
            (width, height),
            dots,
            not_fully_measurable,
            texts,
            rules,
            fallback,
        ),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TextAnchor {
    Start,
    Middle,
    End,
}

impl TextAnchor {
    fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Middle => "middle",
            Self::End => "end",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FigureText<'a> {
    pub x: u32,
    pub y: u32,
    pub text: &'a str,
    pub class: &'static str,
    pub anchor: TextAnchor,
    pub rotation: Option<i16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FigureRule {
    pub x1: u32,
    pub y1: u32,
    pub x2: u32,
    pub y2: u32,
    pub class: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Bar<'a> {
    pub x: u32,
    pub y: u32,
    pub height: u32,
    pub track_width: u32,
    pub value_width: u32,
    pub label: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct StripDot<'a> {
    pub x: u32,
    pub y: u32,
    pub pass_count: usize,
    pub label: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct NotFullyMeasurableMark<'a> {
    pub x: u32,
    pub y: u32,
    pub label: &'a str,
    pub detail: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)] // Brief 8 maps result statuses into these production marks.
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
fn render_status_marks(
    accessibility: FigureAccessibility<'_>,
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
        accessibility,
        width,
        height,
        &body,
        TableFallback {
            headers: &["Item", "Status"],
            rows: &rows,
        },
    )
}

/// Renders prepared 44 px aggregate cells. The caller explicitly selects `no_verdict` per cell.
fn render_aggregate_marks(
    accessibility: FigureAccessibility<'_>,
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
        accessibility,
        width,
        height,
        &body,
        TableFallback {
            headers: &["Item", "Status", "Passed / measured", "Verdict"],
            rows: &rows,
        },
    )
}

fn render_status_raster(
    accessibility: FigureAccessibility<'_>,
    width: u32,
    height: u32,
    marks: &[StatusMark<'_>],
    texts: &[FigureText<'_>],
    rules: &[FigureRule],
    fallback: TableFallback<'_>,
) -> String {
    let mut body = String::new();
    write_rules(&mut body, rules);
    write_texts(&mut body, texts);
    for mark in marks {
        write_status_mark(&mut body, mark);
    }
    render_accessible_figure(accessibility, width, height, &body, fallback)
}

fn render_bars(
    accessibility: FigureAccessibility<'_>,
    width: u32,
    height: u32,
    bars: &[Bar<'_>],
    texts: &[FigureText<'_>],
    rules: &[FigureRule],
    fallback: TableFallback<'_>,
) -> String {
    let mut body = String::new();
    write_rules(&mut body, rules);
    write_texts(&mut body, texts);
    for bar in bars {
        writeln!(
            body,
            "    <g class=\"capability-bar\" aria-label=\"{}\">\n      <title>{}</title>\n      <rect class=\"bar-track\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"var(--design-grey-2, #8c8a85)\" stroke-width=\"1\" />\n      <rect class=\"bar-value\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--design-pass, #00513a)\" stroke=\"var(--design-ink, #1c1c1c)\" stroke-width=\"0.75\" />\n    </g>",
            escape_html(bar.label),
            escape_html(bar.label),
            bar.x,
            bar.y,
            bar.track_width,
            bar.height,
            bar.x,
            bar.y,
            bar.value_width,
            bar.height,
        )
        .expect("write SVG bar");
    }
    render_accessible_figure(accessibility, width, height, &body, fallback)
}

fn render_strip_plot(
    accessibility: FigureAccessibility<'_>,
    size: (u32, u32),
    dots: &[StripDot<'_>],
    not_fully_measurable: &[NotFullyMeasurableMark<'_>],
    texts: &[FigureText<'_>],
    rules: &[FigureRule],
    fallback: TableFallback<'_>,
) -> String {
    let (width, height) = size;
    let mut body = String::new();
    write_rules(&mut body, rules);
    write_texts(&mut body, texts);
    for dot in dots {
        writeln!(
            body,
            "    <g class=\"strip-observation\" aria-label=\"{}\">\n      <title>{}</title>\n      <circle class=\"strip-dot\" data-pass-count=\"{}\" cx=\"{}\" cy=\"{}\" r=\"5\" fill=\"var(--design-pass, #00513a)\" stroke=\"var(--design-ink, #1c1c1c)\" stroke-width=\"1\" />\n    </g>",
            escape_html(dot.label),
            escape_html(dot.label),
            dot.pass_count,
            dot.x,
            dot.y,
        )
        .expect("write SVG strip dot");
    }
    for mark in not_fully_measurable {
        writeln!(
            body,
            "    <g class=\"not-fully-measurable-item\" aria-label=\"{}: {}\">\n      <title>{}: {}</title>",
            escape_html(mark.label),
            escape_html(mark.detail),
            escape_html(mark.label),
            escape_html(mark.detail),
        )
        .expect("write SVG not-fully-measurable mark");
        write_mark_shape(&mut body, MarkState::Error, mark.x, mark.y, DENSE_MARK_SIZE);
        body.push_str("    </g>\n");
    }
    render_accessible_figure(accessibility, width, height, &body, fallback)
}

fn write_status_mark(svg: &mut String, mark: &StatusMark<'_>) {
    writeln!(
        svg,
        "    <g class=\"status-mark\" data-state=\"{}\" aria-label=\"{}: {}\">\n      <title>{}: {}</title>",
        mark.state.label(),
        escape_html(mark.label),
        mark.state.label(),
        escape_html(mark.label),
        mark.state.label(),
    )
    .expect("write SVG status mark");
    write_mark_shape(svg, mark.state, mark.x, mark.y, DENSE_MARK_SIZE);
    svg.push_str("    </g>\n");
}

fn write_texts(svg: &mut String, texts: &[FigureText<'_>]) {
    for text in texts {
        write!(
            svg,
            "    <text class=\"{}\" x=\"{}\" y=\"{}\" text-anchor=\"{}\"",
            text.class,
            text.x,
            text.y,
            text.anchor.as_str(),
        )
        .expect("write SVG text");
        if let Some(rotation) = text.rotation {
            write!(
                svg,
                " transform=\"rotate({rotation} {} {})\"",
                text.x, text.y
            )
            .expect("write SVG text rotation");
        }
        writeln!(svg, ">{}</text>", escape_html(text.text)).expect("write SVG text");
    }
}

fn write_rules(svg: &mut String, rules: &[FigureRule]) {
    for rule in rules {
        writeln!(
            svg,
            "    <line class=\"{}\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"var(--design-grey-3, #d5d2cc)\" stroke-width=\"1\" />",
            rule.class, rule.x1, rule.y1, rule.x2, rule.y2
        )
        .expect("write SVG rule");
    }
}

/// Returns only the aggregate-cell overlay so dense raster renderers cannot apply it implicitly.
fn render_no_verdict_overlay(x: u32, y: u32) -> String {
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
    accessibility: FigureAccessibility<'_>,
    width: u32,
    height: u32,
    body: &str,
    fallback: TableFallback<'_>,
) -> String {
    let mut figure = String::new();
    write!(
        figure,
        "<figure class=\"svg-figure\" data-figure-number=\"{}\">\n  <figcaption>\n    <strong>Figure {}. {}</strong>\n    <span>{}</span>\n    <span class=\"does-not-show\"><strong>What this does not show:</strong> {}</span>\n    <a class=\"figure-method-link\" href=\"#method-limitations\">Method and limitations</a>\n  </figcaption>\n  <div class=\"figure-scroll\">\n  <svg role=\"img\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n    <title>Figure {}. {}</title>\n    <desc>{}</desc>\n{body}  </svg>\n  </div>\n  <table class=\"svg-text-fallback\">\n    <caption>Figure {}. {}</caption>\n    <thead><tr>",
        accessibility.number,
        accessibility.number,
        escape_html(accessibility.title),
        escape_html(accessibility.caption),
        escape_html(accessibility.does_not_show),
        accessibility.number,
        escape_html(accessibility.title),
        escape_html(accessibility.description),
        accessibility.number,
        escape_html(accessibility.title)
    )
    .expect("write SVG figure");
    for header in fallback.headers {
        write!(figure, "<th scope=\"col\">{}</th>", escape_html(header))
            .expect("write SVG fallback");
    }
    figure.push_str("</tr></thead>\n    <tbody>\n");
    for row in fallback.rows {
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
        render_figure(Figure::StatusMarks {
            accessibility: FigureAccessibility {
                number: 1,
                title: "Status figure",
                description: "One prepared status mark.",
                caption: "One status observation.",
                does_not_show: "Repeated-run behavior.",
            },
            width: DENSE_MARK_SIZE,
            height: DENSE_MARK_SIZE,
            marks: &[StatusMark {
                x: 0,
                y: 0,
                label,
                state,
            }],
        })
    }

    fn aggregate(no_verdict: bool) -> String {
        render_figure(Figure::AggregateMarks {
            accessibility: FigureAccessibility {
                number: 2,
                title: "Aggregate figure",
                description: "One prepared aggregate mark.",
                caption: "One aggregate observation.",
                does_not_show: "A ranking.",
            },
            width: AGGREGATE_MARK_SIZE,
            height: AGGREGATE_MARK_SIZE,
            marks: &[AggregateMark {
                x: 0,
                y: 0,
                label: "single call",
                state: MarkState::Pass,
                passed: 1,
                measured: 1,
                no_verdict,
            }],
        })
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
        let style = super::super::html::STYLE;
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
        assert!(style.contains("font-family: \"IBM Plex Sans\", \"Helvetica Neue\""));
        assert!(style.contains("font-family: \"IBM Plex Mono\""));
        assert!(!style.contains("@import"));
        assert!(!style.contains("fonts.googleapis"));
        assert!(!style.contains("fonts.gstatic"));
    }
}
