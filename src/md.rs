use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};
use rusty_mermaid::{Color, Point, Primitive, Style, TextStyle};
use rusty_mermaid_core::PathSegment;

pub fn show(ui: &mut Ui, cache: &mut egui_commonmark::CommonMarkCache, markdown: &str) {
    if markdown.is_empty() {
        return;
    }
    for block in split_mermaid(markdown) {
        match block {
            MdBlock::Markdown(segment) => {
                egui_commonmark::CommonMarkViewer::new()
                    .max_image_width(Some(ui.available_width().max(1.0) as usize))
                    .show(ui, cache, segment);
            }
            MdBlock::Mermaid(source) => show_mermaid(ui, source),
        }
    }
}

#[derive(Debug, PartialEq)]
enum MdBlock<'a> {
    Markdown(&'a str),
    Mermaid(&'a str),
}

fn split_mermaid(text: &str) -> Vec<MdBlock<'_>> {
    let mut blocks = Vec::new();
    let mut md_start = 0;
    let mut i = 0;
    while i < text.len() {
        let line_start = i;
        let (line, next) = line_at(text, i);
        if let Some((fence_char, fence_len)) = open_fence(line) {
            if let Some((content_end, after_close)) = find_close(text, next, fence_char, fence_len)
            {
                push_md(&mut blocks, text, md_start, line_start);
                let inner = trim_one_trailing_newline(&text[next..content_end]);
                blocks.push(MdBlock::Mermaid(inner));
                md_start = after_close;
                i = after_close;
                continue;
            }
            break;
        }
        i = next;
    }
    push_md(&mut blocks, text, md_start, text.len());
    if blocks.is_empty() {
        blocks.push(MdBlock::Markdown(text));
    }
    blocks
}

fn line_at(text: &str, start: usize) -> (&str, usize) {
    let rest = &text[start..];
    let (raw, next) = match rest.find('\n') {
        Some(n) => (&rest[..n], start + n + 1),
        None => (rest, text.len()),
    };
    (raw.strip_suffix('\r').unwrap_or(raw), next)
}

fn open_fence(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start_matches(' ');
    let indent = line.len() - trimmed.len();
    if indent > 3 {
        return None;
    }
    let fence_char = trimmed.chars().next()?;
    if fence_char != '`' && fence_char != '~' {
        return None;
    }
    let fence_len = trimmed
        .bytes()
        .take_while(|b| *b == fence_char as u8)
        .count();
    if fence_len < 3 {
        return None;
    }
    let info = trimmed[fence_len..].trim();
    let token = info.split_whitespace().next().unwrap_or("");
    if token.eq_ignore_ascii_case("mermaid") {
        Some((fence_char, fence_len))
    } else {
        None
    }
}

fn find_close(text: &str, from: usize, fence_char: char, min_len: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while i < text.len() {
        let line_start = i;
        let (line, next) = line_at(text, i);
        if is_close(line, fence_char, min_len) {
            return Some((line_start, next));
        }
        i = next;
    }
    None
}

fn is_close(line: &str, fence_char: char, min_len: usize) -> bool {
    let trimmed = line.trim();
    let mut len = 0;
    for c in trimmed.chars() {
        if c != fence_char {
            return false;
        }
        len += 1;
    }
    len >= min_len
}

fn trim_one_trailing_newline(s: &str) -> &str {
    let s = s.strip_suffix('\n').unwrap_or(s);
    s.strip_suffix('\r').unwrap_or(s)
}

fn push_md<'a>(blocks: &mut Vec<MdBlock<'a>>, text: &'a str, start: usize, end: usize) {
    if start < end {
        blocks.push(MdBlock::Markdown(&text[start..end]));
    }
}

fn space_arrows(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(chars.len() + 8);
    let mut depth = 0i32;
    let mut quote = None;
    for (i, &c) in chars.iter().enumerate() {
        if let Some(q) = quote {
            out.push(c);
            if c == q {
                quote = None;
            }
            continue;
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
            out.push(c);
            continue;
        }
        match c {
            '[' | '{' | '(' => depth += 1,
            ']' | '}' | ')' if depth > 0 => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            let prev = i.checked_sub(1).map(|j| chars[j]).unwrap_or('\0');
            let next = chars.get(i + 1).copied().unwrap_or('\0');
            let prev_glued = prev.is_ascii_alphanumeric() || matches!(prev, '_' | ']' | ')' | '}');
            let starts_arrow = matches!(
                (c, next),
                ('-' | '=', '-' | '.' | '>' | '=') | ('<', '-' | '>')
            );
            if prev_glued && starts_arrow {
                out.push(' ');
            }
            out.push(c);
            if c == '>' && next.is_ascii_alphanumeric() {
                out.push(' ');
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn show_mermaid(ui: &mut Ui, source: &str) {
    let source = source.trim();
    if source.is_empty() {
        return;
    }
    let scene = match rusty_mermaid::render(&space_arrows(source), &rusty_mermaid::Theme::light()) {
        Ok(scene) => scene,
        Err(e) => {
            ui.weak(e.to_string());
            ui.add(egui::Label::new(RichText::new(source).monospace()).wrap());
            return;
        }
    };
    if !scene.width.is_finite()
        || !scene.height.is_finite()
        || scene.width <= 1.0
        || scene.height <= 1.0
    {
        return;
    }
    let available = ui.available_width();
    if available <= 0.0 {
        return;
    }
    let scale = (available / scene.width as f32).min(1.0);
    if !scale.is_finite() || scale <= 0.0 {
        return;
    }
    let size = egui::vec2(scene.width as f32 * scale, scene.height as f32 * scale);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    let view = SceneView {
        origin: rect.min,
        scale,
        xform: Affine::identity(),
    };
    for element in scene.elements() {
        paint_primitive(&painter, &view, &element.primitive);
    }
}

#[derive(Clone, Copy)]
struct Affine {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Affine {
    fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    fn translate(x: f64, y: f64) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: x as f32,
            f: y as f32,
        }
    }

    fn scale(sx: f64, sy: f64) -> Self {
        Self {
            a: sx as f32,
            b: 0.0,
            c: 0.0,
            d: sy as f32,
            e: 0.0,
            f: 0.0,
        }
    }

    fn rotate(degrees: f64, cx: f64, cy: f64) -> Self {
        let r = degrees.to_radians();
        let (s, c) = (r.sin() as f32, r.cos() as f32);
        let rot = Self {
            a: c,
            b: s,
            c: -s,
            d: c,
            e: 0.0,
            f: 0.0,
        };
        Self::translate(cx, cy)
            .mul(rot)
            .mul(Self::translate(-cx, -cy))
    }

    fn mul(self, rhs: Self) -> Self {
        Self {
            a: self.a * rhs.a + self.c * rhs.b,
            b: self.b * rhs.a + self.d * rhs.b,
            c: self.a * rhs.c + self.c * rhs.d,
            d: self.b * rhs.c + self.d * rhs.d,
            e: self.a * rhs.e + self.c * rhs.f + self.e,
            f: self.b * rhs.e + self.d * rhs.f + self.f,
        }
    }

    fn apply(self, x: f64, y: f64) -> (f32, f32) {
        let x = x as f32;
        let y = y as f32;
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    fn from_transform(t: rusty_mermaid_core::Transform) -> Self {
        match t {
            rusty_mermaid_core::Transform::Identity => Self::identity(),
            rusty_mermaid_core::Transform::Translate(x, y) => Self::translate(x, y),
            rusty_mermaid_core::Transform::Scale(sx, sy) => Self::scale(sx, sy),
            rusty_mermaid_core::Transform::Rotate { degrees, cx, cy } => {
                Self::rotate(degrees, cx, cy)
            }
        }
    }
}

struct SceneView {
    origin: Pos2,
    scale: f32,
    xform: Affine,
}

impl SceneView {
    fn with_transform(&self, t: rusty_mermaid_core::Transform) -> Self {
        Self {
            origin: self.origin,
            scale: self.scale,
            xform: self.xform.mul(Affine::from_transform(t)),
        }
    }

    fn screen(&self, p: Point) -> Pos2 {
        let (x, y) = self.xform.apply(p.x, p.y);
        Pos2::new(
            self.origin.x + x * self.scale,
            self.origin.y + y * self.scale,
        )
    }

    fn scale_x(&self) -> f32 {
        self.scale * self.xform.a.hypot(self.xform.b)
    }

    fn scale_y(&self) -> f32 {
        self.scale * self.xform.c.hypot(self.xform.d)
    }

    fn uniform_scale(&self) -> f32 {
        self.scale_x()
    }

    fn axis_aligned(&self) -> bool {
        self.xform.b.abs() < 1e-4 && self.xform.c.abs() < 1e-4
    }
}

fn paint_primitive(painter: &egui::Painter, view: &SceneView, primitive: &Primitive) {
    match primitive {
        Primitive::Rect {
            bbox,
            rx,
            ry,
            style,
        } => paint_rect(painter, view, *bbox, *rx, *ry, style),
        Primitive::Circle {
            center,
            radius,
            style,
        } => {
            paint_ellipse(painter, view, *center, *radius, *radius, style);
        }
        Primitive::Ellipse {
            center,
            rx,
            ry,
            style,
        } => {
            paint_ellipse(painter, view, *center, *rx, *ry, style);
        }
        Primitive::Polygon { points, style } => paint_polygon(painter, view, points, style, true),
        Primitive::Path {
            segments,
            style,
            marker_start,
            marker_end,
        } => paint_path(painter, view, segments, style, *marker_start, *marker_end),
        Primitive::Text {
            position,
            content,
            style,
            ..
        } => paint_text(painter, view, *position, content, style),
        Primitive::Group {
            transform,
            children,
        } => {
            let child_view = view.with_transform(*transform);
            for child in children {
                paint_primitive(painter, &child_view, child);
            }
        }
        Primitive::Arc {
            center,
            inner_r,
            outer_r,
            start_angle,
            end_angle,
            style,
        } => paint_arc(
            painter,
            view,
            *center,
            *inner_r,
            *outer_r,
            *start_angle,
            *end_angle,
            style,
        ),
    }
}

fn color32(c: Color, opacity: Option<f64>) -> Color32 {
    let mut a = c.a as f32;
    if let Some(op) = opacity {
        a *= op.clamp(0.0, 1.0) as f32;
    }
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, a.round().clamp(0.0, 255.0) as u8)
}

fn fill_color(style: &Style) -> Color32 {
    style
        .fill
        .map(|c| color32(c, style.opacity))
        .unwrap_or(Color32::TRANSPARENT)
}

fn stroke_of(style: &Style, width_scale: f32) -> Stroke {
    let Some(c) = style.stroke else {
        return Stroke::NONE;
    };
    let w = style.stroke_width.unwrap_or(1.0) as f32 * width_scale;
    if w <= 0.0 {
        Stroke::NONE
    } else {
        Stroke::new(w, color32(c, style.opacity))
    }
}

fn paint_rect(
    painter: &egui::Painter,
    view: &SceneView,
    bbox: rusty_mermaid::BBox,
    rx: f64,
    ry: f64,
    style: &Style,
) {
    let fill = fill_color(style);
    let stroke = stroke_of(style, view.uniform_scale());
    if view.axis_aligned() {
        let center = view.screen(Point::new(bbox.x, bbox.y));
        let size = Vec2::new(
            (bbox.width as f32 * view.scale_x()).abs(),
            (bbox.height as f32 * view.scale_y()).abs(),
        );
        let rect = Rect::from_center_size(center, size);
        let radius = (rx.min(ry) as f32 * view.uniform_scale()).clamp(0.0, 255.0);
        painter.rect(rect, radius, fill, stroke, StrokeKind::Middle);
        return;
    }
    let hw = bbox.width / 2.0;
    let hh = bbox.height / 2.0;
    let corners = [
        Point::new(bbox.x - hw, bbox.y - hh),
        Point::new(bbox.x + hw, bbox.y - hh),
        Point::new(bbox.x + hw, bbox.y + hh),
        Point::new(bbox.x - hw, bbox.y + hh),
    ];
    let points: Vec<Pos2> = corners.iter().copied().map(|p| view.screen(p)).collect();
    painter.add(Shape::convex_polygon(points, fill, stroke));
}

fn paint_ellipse(
    painter: &egui::Painter,
    view: &SceneView,
    center: Point,
    rx: f64,
    ry: f64,
    style: &Style,
) {
    let fill = fill_color(style);
    let stroke = stroke_of(style, view.uniform_scale());
    if view.axis_aligned() {
        painter.add(egui::epaint::EllipseShape {
            center: view.screen(center),
            radius: Vec2::new(
                (rx as f32 * view.scale_x()).abs(),
                (ry as f32 * view.scale_y()).abs(),
            ),
            fill,
            stroke,
        });
        return;
    }
    let points = ellipse_points(center, rx, ry, 24);
    paint_polygon(painter, view, &points, style, true);
}

fn ellipse_points(center: Point, rx: f64, ry: f64, steps: usize) -> Vec<Point> {
    let steps = steps.max(3);
    (0..steps)
        .map(|i| {
            let t = std::f64::consts::TAU * (i as f64) / (steps as f64);
            Point::new(center.x + rx * t.cos(), center.y + ry * t.sin())
        })
        .collect()
}

fn paint_polygon(
    painter: &egui::Painter,
    view: &SceneView,
    points: &[Point],
    style: &Style,
    closed: bool,
) {
    if points.len() < 2 {
        return;
    }
    let screen: Vec<Pos2> = points.iter().copied().map(|p| view.screen(p)).collect();
    let fill = if closed {
        fill_color(style)
    } else {
        Color32::TRANSPARENT
    };
    let stroke = stroke_of(style, view.uniform_scale());
    if closed && screen.len() >= 3 {
        painter.add(Shape::convex_polygon(screen, fill, stroke));
    } else if stroke != Stroke::NONE {
        painter.line(screen, stroke);
    }
}

fn paint_arc(
    painter: &egui::Painter,
    view: &SceneView,
    center: Point,
    inner_r: f64,
    outer_r: f64,
    start: f64,
    end: f64,
    style: &Style,
) {
    let steps = 24;
    let mut points = Vec::with_capacity(steps + 2);
    for i in 0..=steps {
        let t = start + (end - start) * (i as f64) / (steps as f64);
        points.push(Point::new(
            center.x + outer_r * t.cos(),
            center.y + outer_r * t.sin(),
        ));
    }
    if inner_r > 0.5 {
        for i in (0..=steps).rev() {
            let t = start + (end - start) * (i as f64) / (steps as f64);
            points.push(Point::new(
                center.x + inner_r * t.cos(),
                center.y + inner_r * t.sin(),
            ));
        }
    } else {
        points.push(center);
    }
    paint_polygon(painter, view, &points, style, true);
}

fn paint_path(
    painter: &egui::Painter,
    view: &SceneView,
    segments: &[PathSegment],
    style: &Style,
    marker_start: Option<rusty_mermaid_core::MarkerType>,
    marker_end: Option<rusty_mermaid_core::MarkerType>,
) {
    let fill = fill_color(style);
    if fill != Color32::TRANSPARENT {
        for sub in flatten_subpaths(view, segments) {
            if sub.len() >= 3 {
                painter.add(Shape::convex_polygon(sub, fill, Stroke::NONE));
            }
        }
    }
    stroke_segments(painter, view, segments, style);
    let sw = style.stroke_width.unwrap_or(1.5);
    let marker_color = style
        .stroke
        .map(|c| color32(c, style.opacity))
        .unwrap_or(Color32::from_rgb(0x33, 0x33, 0x33));
    if let Some(marker) = marker_end
        && let Some((tip, angle)) = rusty_mermaid_core::path_end_tangent(segments)
    {
        paint_marker(painter, view, marker, tip, angle, sw, marker_color);
    }
    if let Some(marker) = marker_start
        && let Some((tip, angle)) = rusty_mermaid_core::path_start_tangent(segments)
    {
        paint_marker(painter, view, marker, tip, angle, sw, marker_color);
    }
}

fn paint_marker(
    painter: &egui::Painter,
    view: &SceneView,
    marker: rusty_mermaid_core::MarkerType,
    tip: Point,
    angle: f64,
    stroke_width: f64,
    color: Color32,
) {
    use rusty_mermaid_core::MarkerPath;
    let path = rusty_mermaid_core::marker_path(marker, tip, angle, stroke_width);
    match path {
        MarkerPath::FillPolygon { points } => {
            let pts = points.iter().copied().map(|p| view.screen(p)).collect();
            painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        MarkerPath::StrokePolyline {
            points,
            width,
            closed,
        } => {
            let pts: Vec<Pos2> = points.iter().copied().map(|p| view.screen(p)).collect();
            let stroke = Stroke::new((width as f32 * view.uniform_scale()).max(1.0), color);
            if closed && pts.len() >= 3 {
                painter.add(Shape::closed_line(pts, stroke));
            } else if pts.len() >= 2 {
                painter.line(pts, stroke);
            }
        }
        MarkerPath::FillAndStrokePolygon {
            points,
            stroke_width,
            fill_is_marker_color,
        } => {
            let pts = points.iter().copied().map(|p| view.screen(p)).collect();
            let fill = if fill_is_marker_color {
                color
            } else {
                Color32::WHITE
            };
            let stroke = Stroke::new((stroke_width as f32 * view.uniform_scale()).max(1.0), color);
            painter.add(Shape::convex_polygon(pts, fill, stroke));
        }
        MarkerPath::FillCircle { center, radius } => {
            painter.circle_filled(
                view.screen(center),
                (radius as f32 * view.uniform_scale()).abs(),
                color,
            );
        }
        MarkerPath::StrokeCurves { curves, width } => {
            let stroke = Stroke::new((width as f32 * view.uniform_scale()).max(1.0), color);
            for [from, cp, to] in curves {
                let (cp1, cp2) = quad_to_cubic(from, cp, to);
                painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                    [
                        view.screen(from),
                        view.screen(cp1),
                        view.screen(cp2),
                        view.screen(to),
                    ],
                    false,
                    Color32::TRANSPARENT,
                    stroke,
                ));
            }
        }
    }
}

fn stroke_segments(
    painter: &egui::Painter,
    view: &SceneView,
    segments: &[PathSegment],
    style: &Style,
) {
    let Some(c) = style.stroke else {
        return;
    };
    let color = color32(c, style.opacity);
    let width = style.stroke_width.unwrap_or(1.0) as f32 * view.uniform_scale();
    if width <= 0.0 {
        return;
    }
    let dash = style.stroke_dasharray.as_deref();
    let mut current = Point::new(0.0, 0.0);
    let mut sub_start = current;
    for seg in segments {
        match *seg {
            PathSegment::MoveTo(p) => {
                current = p;
                sub_start = p;
            }
            PathSegment::LineTo(p) => {
                stroke_line(painter, view, current, p, color, width, dash);
                current = p;
            }
            PathSegment::CubicTo { cp1, cp2, to } => {
                stroke_cubic(painter, view, current, cp1, cp2, to, color, width, dash);
                current = to;
            }
            PathSegment::QuadTo { cp, to } => {
                let (cp1, cp2) = quad_to_cubic(current, cp, to);
                stroke_cubic(painter, view, current, cp1, cp2, to, color, width, dash);
                current = to;
            }
            PathSegment::ArcTo {
                rx,
                ry,
                rotation,
                large_arc,
                sweep,
                to,
            } => {
                let pts = arc_points(current, to, rx, ry, rotation, large_arc, sweep, 24);
                let mut prev = current;
                for p in pts {
                    stroke_line(painter, view, prev, p, color, width, dash);
                    prev = p;
                }
                current = to;
            }
            PathSegment::Close => {
                stroke_line(painter, view, current, sub_start, color, width, dash);
                current = sub_start;
            }
        }
    }
}

fn stroke_line(
    painter: &egui::Painter,
    view: &SceneView,
    from: Point,
    to: Point,
    color: Color32,
    width: f32,
    dash: Option<&[f64]>,
) {
    let a = view.screen(from);
    let b = view.screen(to);
    stroke_screen_line(painter, a, b, color, width, dash, view.uniform_scale());
}

fn stroke_cubic(
    painter: &egui::Painter,
    view: &SceneView,
    from: Point,
    cp1: Point,
    cp2: Point,
    to: Point,
    color: Color32,
    width: f32,
    dash: Option<&[f64]>,
) {
    if dash.is_some() {
        let pts = sample_cubic(from, cp1, cp2, to, 16);
        let mut prev = from;
        for p in pts {
            stroke_line(painter, view, prev, p, color, width, dash);
            prev = p;
        }
        return;
    }
    painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
        [
            view.screen(from),
            view.screen(cp1),
            view.screen(cp2),
            view.screen(to),
        ],
        false,
        Color32::TRANSPARENT,
        Stroke::new(width, color),
    ));
}

fn stroke_screen_line(
    painter: &egui::Painter,
    from: Pos2,
    to: Pos2,
    color: Color32,
    width: f32,
    dash: Option<&[f64]>,
    scale: f32,
) {
    let Some(pattern) = dash else {
        painter.line_segment([from, to], Stroke::new(width, color));
        return;
    };
    let pattern: Vec<f32> = pattern
        .iter()
        .map(|d| (*d as f32 * scale).max(0.4))
        .collect();
    if pattern.is_empty() {
        painter.line_segment([from, to], Stroke::new(width, color));
        return;
    }
    let mut dist = from.distance(to);
    if dist < 1e-3 {
        return;
    }
    let dir = (to - from) / dist;
    let mut cursor = from;
    let mut on = true;
    let mut pat_i = 0;
    let mut remain = pattern[0];
    while dist > 0.05 {
        let step = remain.min(dist);
        let next = cursor + dir * step;
        if on {
            painter.line_segment([cursor, next], Stroke::new(width, color));
        }
        cursor = next;
        dist -= step;
        remain -= step;
        if remain <= 0.05 {
            pat_i = (pat_i + 1) % pattern.len();
            remain = pattern[pat_i];
            on = !on;
        }
    }
}

fn quad_to_cubic(from: Point, cp: Point, to: Point) -> (Point, Point) {
    let cp1 = Point::new(
        from.x + (cp.x - from.x) * 2.0 / 3.0,
        from.y + (cp.y - from.y) * 2.0 / 3.0,
    );
    let cp2 = Point::new(
        to.x + (cp.x - to.x) * 2.0 / 3.0,
        to.y + (cp.y - to.y) * 2.0 / 3.0,
    );
    (cp1, cp2)
}

fn sample_cubic(p0: Point, p1: Point, p2: Point, p3: Point, steps: usize) -> Vec<Point> {
    (1..=steps)
        .map(|i| {
            let t = i as f64 / steps as f64;
            let u = 1.0 - t;
            Point::new(
                u * u * u * p0.x
                    + 3.0 * u * u * t * p1.x
                    + 3.0 * u * t * t * p2.x
                    + t * t * t * p3.x,
                u * u * u * p0.y
                    + 3.0 * u * u * t * p1.y
                    + 3.0 * u * t * t * p2.y
                    + t * t * t * p3.y,
            )
        })
        .collect()
}

fn flatten_subpaths(view: &SceneView, segments: &[PathSegment]) -> Vec<Vec<Pos2>> {
    let mut out = Vec::new();
    let mut current = Point::new(0.0, 0.0);
    let mut sub_start = current;
    let mut sub = Vec::new();
    let flush = |sub: &mut Vec<Pos2>, out: &mut Vec<Vec<Pos2>>| {
        if sub.len() >= 2 {
            out.push(std::mem::take(sub));
        } else {
            sub.clear();
        }
    };
    for seg in segments {
        match *seg {
            PathSegment::MoveTo(p) => {
                flush(&mut sub, &mut out);
                current = p;
                sub_start = p;
                sub.push(view.screen(p));
            }
            PathSegment::LineTo(p) => {
                if sub.is_empty() {
                    sub.push(view.screen(current));
                }
                sub.push(view.screen(p));
                current = p;
            }
            PathSegment::CubicTo { cp1, cp2, to } => {
                if sub.is_empty() {
                    sub.push(view.screen(current));
                }
                for p in sample_cubic(current, cp1, cp2, to, 8) {
                    sub.push(view.screen(p));
                }
                current = to;
            }
            PathSegment::QuadTo { cp, to } => {
                let (cp1, cp2) = quad_to_cubic(current, cp, to);
                if sub.is_empty() {
                    sub.push(view.screen(current));
                }
                for p in sample_cubic(current, cp1, cp2, to, 8) {
                    sub.push(view.screen(p));
                }
                current = to;
            }
            PathSegment::ArcTo {
                rx,
                ry,
                rotation,
                large_arc,
                sweep,
                to,
            } => {
                if sub.is_empty() {
                    sub.push(view.screen(current));
                }
                for p in arc_points(current, to, rx, ry, rotation, large_arc, sweep, 24) {
                    sub.push(view.screen(p));
                }
                current = to;
            }
            PathSegment::Close => {
                sub.push(view.screen(sub_start));
                flush(&mut sub, &mut out);
                current = sub_start;
            }
        }
    }
    flush(&mut sub, &mut out);
    out
}

/// SVG elliptical arc, sampled in scene space. Endpoint is `to`.
fn arc_points(
    from: Point,
    to: Point,
    mut rx: f64,
    mut ry: f64,
    rotation_deg: f64,
    large_arc: bool,
    sweep: bool,
    steps: usize,
) -> Vec<Point> {
    if (from.x - to.x).abs() < 1e-6 && (from.y - to.y).abs() < 1e-6 {
        return vec![to];
    }
    rx = rx.abs();
    ry = ry.abs();
    if rx < 1e-6 || ry < 1e-6 {
        return vec![to];
    }
    let phi = rotation_deg.to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();
    let dx = (from.x - to.x) / 2.0;
    let dy = (from.y - to.y) / 2.0;
    let x1p = cos_phi * dx + sin_phi * dy;
    let y1p = -sin_phi * dx + cos_phi * dy;
    let mut rx2 = rx * rx;
    let mut ry2 = ry * ry;
    let lambda = x1p * x1p / rx2 + y1p * y1p / ry2;
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
        rx2 = rx * rx;
        ry2 = ry * ry;
    }
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let sq = ((rx2 * ry2 - rx2 * y1p * y1p - ry2 * x1p * x1p)
        / (rx2 * y1p * y1p + ry2 * x1p * x1p))
        .max(0.0);
    let coef = sign * sq.sqrt();
    let cxp = coef * rx * y1p / ry;
    let cyp = coef * -ry * x1p / rx;
    let cx = cos_phi * cxp - sin_phi * cyp + (from.x + to.x) / 2.0;
    let cy = sin_phi * cxp + cos_phi * cyp + (from.y + to.y) / 2.0;
    let theta1 = vector_angle(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dtheta = vector_angle(
        (x1p - cxp) / rx,
        (y1p - cyp) / ry,
        (-x1p - cxp) / rx,
        (-y1p - cyp) / ry,
    );
    if !sweep && dtheta > 0.0 {
        dtheta -= std::f64::consts::TAU;
    } else if sweep && dtheta < 0.0 {
        dtheta += std::f64::consts::TAU;
    }
    let steps = steps.max(1);
    (1..=steps)
        .map(|i| {
            let theta = theta1 + dtheta * (i as f64) / (steps as f64);
            let (cos_t, sin_t) = (theta.cos(), theta.sin());
            Point::new(
                cos_phi * rx * cos_t - sin_phi * ry * sin_t + cx,
                sin_phi * rx * cos_t + cos_phi * ry * sin_t + cy,
            )
        })
        .collect()
}

fn vector_angle(ux: f64, uy: f64, vx: f64, vy: f64) -> f64 {
    let dot = ux * vx + uy * vy;
    let len = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
    if len < 1e-12 {
        return 0.0;
    }
    let mut ang = (dot / len).clamp(-1.0, 1.0).acos();
    if ux * vy - uy * vx < 0.0 {
        ang = -ang;
    }
    ang
}

fn paint_text(
    painter: &egui::Painter,
    view: &SceneView,
    position: Point,
    content: &str,
    style: &TextStyle,
) {
    let size = style.font_size as f32 * view.uniform_scale();
    if content.is_empty() || size < 0.5 {
        return;
    }
    let color = style
        .fill
        .map(|c| color32(c, None))
        .unwrap_or(Color32::from_rgb(0x33, 0x33, 0x33));
    let pos = view.screen(position);
    let font = FontId::proportional(size);
    if let Some(spans) = rusty_mermaid_core::parse_inline_markdown(content) {
        paint_spans(painter, pos, &spans, &font, color);
    } else {
        painter.text(pos, Align2::CENTER_CENTER, content, font, color);
    }
}

fn paint_spans(
    painter: &egui::Painter,
    pos: Pos2,
    spans: &[rusty_mermaid_core::MdSpan],
    font: &FontId,
    color: Color32,
) {
    let mut widths = Vec::with_capacity(spans.len());
    let mut total = 0.0;
    for span in spans {
        let w = if span.text.is_empty() {
            0.0
        } else {
            measure_span(painter, &span.text, font, color, span.italic)
        };
        widths.push(w);
        total += w;
    }
    let mut x = pos.x - total * 0.5;
    for (span, w) in spans.iter().zip(widths) {
        if !span.text.is_empty() {
            paint_styled(
                painter,
                Pos2::new(x + w * 0.5, pos.y),
                &span.text,
                font,
                color,
                span.bold,
                span.italic,
            );
        }
        x += w;
    }
}

fn measure_span(
    painter: &egui::Painter,
    text: &str,
    font: &FontId,
    color: Color32,
    italic: bool,
) -> f32 {
    if italic {
        layout_italic(painter, text, font, color).size().x
    } else {
        painter
            .layout_no_wrap(text.to_string(), font.clone(), color)
            .size()
            .x
    }
}

fn paint_styled(
    painter: &egui::Painter,
    pos: Pos2,
    text: &str,
    font: &FontId,
    color: Color32,
    bold: bool,
    italic: bool,
) {
    if italic {
        let galley = layout_italic(painter, text, font, color);
        let rect = Align2::CENTER_CENTER.anchor_size(pos, galley.size());
        painter.galley(rect.min, galley.clone(), color);
        if bold {
            painter.galley(rect.min + egui::vec2(0.6, 0.0), galley, color);
        }
        return;
    }
    painter.text(pos, Align2::CENTER_CENTER, text, font.clone(), color);
    if bold {
        painter.text(
            pos + egui::vec2(0.6, 0.0),
            Align2::CENTER_CENTER,
            text,
            font.clone(),
            color,
        );
    }
}

fn layout_italic(
    painter: &egui::Painter,
    text: &str,
    font: &FontId,
    color: Color32,
) -> std::sync::Arc<egui::text::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::text::TextFormat {
            font_id: font.clone(),
            color,
            italics: true,
            ..Default::default()
        },
    );
    painter.layout_job(job)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_closed_mermaid_fence() {
        let md = "# 題\n\n```mermaid\nflowchart TD\n  A-->B\n```\n\nafter\n";
        let blocks = split_mermaid(md);
        assert_eq!(blocks.len(), 3);
        match &blocks[0] {
            MdBlock::Markdown(s) => assert!(s.starts_with("# 題"), "{s:?}"),
            MdBlock::Mermaid(_) => panic!("first block should be markdown"),
        }
        assert_eq!(blocks[1], MdBlock::Mermaid("flowchart TD\n  A-->B"));
        match &blocks[2] {
            MdBlock::Markdown(s) => assert!(s.contains("after"), "{s:?}"),
            MdBlock::Mermaid(_) => panic!("third block should be markdown"),
        }
    }

    #[test]
    fn unclosed_mermaid_stays_markdown() {
        let md = "```mermaid\nflowchart TD\n  A-->B\n";
        let blocks = split_mermaid(md);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::Markdown(s) => assert!(s.contains("flowchart"), "{s:?}"),
            MdBlock::Mermaid(_) => panic!("unclosed fence must stay markdown"),
        }
    }

    #[test]
    fn rust_fence_is_not_mermaid() {
        let md = "```rust\nfn main() {}\n```\n";
        let blocks = split_mermaid(md);
        assert!(blocks.iter().all(|b| matches!(b, MdBlock::Markdown(_))));
        assert_eq!(
            blocks
                .iter()
                .filter(|b| matches!(b, MdBlock::Mermaid(_)))
                .count(),
            0
        );
    }

    #[test]
    fn indented_fence_stays_markdown() {
        let md = "    ```mermaid\n    flowchart TD\n      A-->B\n    ```\n";
        let blocks = split_mermaid(md);
        assert!(blocks.iter().all(|b| matches!(b, MdBlock::Markdown(_))));
    }

    #[test]
    fn flowchart_scene_has_size() {
        let scene = rusty_mermaid::render("flowchart TD\n  A-->B", &rusty_mermaid::Theme::light())
            .expect("flowchart");
        assert!(scene.width > 1.0);
    }

    #[test]
    fn mermaid_adds_paint_shapes() {
        let plain = "# 題\n\n**bold**\n\n[link](https://github.com)\n\nafter\n";
        let with_fig = "# 題\n\n**bold**\n\n[link](https://github.com)\n\n```mermaid\nflowchart TD\n  A-->B\n```\n\nafter\n";
        let plain_n = paint_shapes(plain).len();
        let fig = paint_shapes(with_fig);
        assert!(
            fig.len() > plain_n,
            "mermaid should add shapes: plain={plain_n} with={}",
            fig.len()
        );
        let text = painted_text(&fig);
        assert!(text.contains("bold"), "{text}");
        assert!(text.contains("after"), "{text}");
        assert!(text.contains('A'), "{text}");
        assert!(text.contains('B'), "{text}");
        assert!(
            !text.contains("-->"),
            "source arrow must not remain: {text}"
        );
        assert!(
            !text.contains("flowchart"),
            "source fence must not remain: {text}"
        );
    }

    fn paint_shapes(markdown: &str) -> Vec<egui::epaint::ClippedShape> {
        let ctx = egui::Context::default();
        let mut cache = egui_commonmark::CommonMarkCache::default();
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 1600.0),
        ));
        ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                show(ui, &mut cache, markdown);
            });
        })
        .shapes
    }

    fn painted_text(shapes: &[egui::epaint::ClippedShape]) -> String {
        let mut out = String::new();
        for clipped in shapes {
            collect_text(&clipped.shape, &mut out);
        }
        out
    }

    fn collect_text(shape: &egui::Shape, out: &mut String) {
        match shape {
            egui::Shape::Text(text) => out.push_str(text.galley.text()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_text(shape, out);
                }
            }
            _ => {}
        }
    }
}
