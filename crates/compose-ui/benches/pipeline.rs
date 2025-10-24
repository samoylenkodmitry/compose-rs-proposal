use compose_core::{location_key, Composition, Key, MemoryApplier};
use compose_ui::{
    composable, measure_layout, Column, ColumnSpec, HeadlessRenderer, LayoutMeasurements, Modifier,
    Row, RowSpec, Size, Text,
};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

const SECTION_COUNT: usize = 4;
const ROWS_PER_SECTION: usize = 32;
const ROOT_SIZE: Size = Size {
    width: 1080.0,
    height: 1920.0,
};

#[composable]
fn pipeline_content(sections: usize, rows_per_section: usize) {
    Column(
        Modifier::fill_max_size(),
        ColumnSpec::default(),
        move || {
            for section in 0..sections {
                Column(
                    Modifier::fill_max_width(),
                    ColumnSpec::default(),
                    move || {
                        Text(format!("Section {section}"), Modifier::empty());
                        for row in 0..rows_per_section {
                            Row(Modifier::fill_max_width(), RowSpec::default(), move || {
                                Text(format!("Item {section}-{row} title"), Modifier::weight(1.0));
                                Text(format!("Detail {section}-{row}"), Modifier::empty());
                            });
                        }
                    },
                );
            }
        },
    );
}

struct PipelineFixture {
    composition: Composition<MemoryApplier>,
    key: Key,
    sections: usize,
    rows: usize,
    root_size: Size,
}

impl PipelineFixture {
    fn new(sections: usize, rows: usize, root_size: Size) -> Self {
        let key = location_key(file!(), line!(), column!());
        Self {
            composition: Composition::new(MemoryApplier::new()),
            key,
            sections,
            rows,
            root_size,
        }
    }

    fn compose(&mut self) {
        let sections = self.sections;
        let rows = self.rows;
        self.composition
            .render(self.key, || pipeline_content(sections, rows))
            .expect("composition");
    }

    fn measure(&mut self) -> LayoutMeasurements {
        let root = self.composition.root().expect("composition root");
        measure_layout(self.composition.applier_mut(), root, self.root_size).expect("measure")
    }
}

fn bench_composition(c: &mut Criterion) {
    let mut fixture = PipelineFixture::new(SECTION_COUNT, ROWS_PER_SECTION, ROOT_SIZE);
    // Warm up the composition so steady-state recomposition is measured.
    fixture.compose();

    c.bench_function("pipeline_composition", |b| {
        b.iter(|| {
            fixture.compose();
        });
    });
}

fn bench_measure(c: &mut Criterion) {
    let mut fixture = PipelineFixture::new(SECTION_COUNT, ROWS_PER_SECTION, ROOT_SIZE);
    fixture.compose();

    c.bench_function("pipeline_measure", |b| {
        b.iter(|| {
            let measurements = fixture.measure();
            black_box(measurements);
        });
    });
}

fn bench_layout(c: &mut Criterion) {
    let mut fixture = PipelineFixture::new(SECTION_COUNT, ROWS_PER_SECTION, ROOT_SIZE);
    fixture.compose();
    let measurements = fixture.measure();

    c.bench_function("pipeline_layout", |b| {
        b.iter(|| {
            let tree = measurements.layout_tree();
            black_box(tree);
        });
    });
}

fn bench_render(c: &mut Criterion) {
    let mut fixture = PipelineFixture::new(SECTION_COUNT, ROWS_PER_SECTION, ROOT_SIZE);
    fixture.compose();
    let measurements = fixture.measure();
    let layout_tree = measurements.layout_tree();
    let renderer = HeadlessRenderer::new();

    c.bench_function("pipeline_render", |b| {
        b.iter(|| {
            let scene = renderer.render(&layout_tree);
            black_box(scene);
        });
    });
}

fn bench_full_pipeline(c: &mut Criterion) {
    let mut fixture = PipelineFixture::new(SECTION_COUNT, ROWS_PER_SECTION, ROOT_SIZE);
    let renderer = HeadlessRenderer::new();

    c.bench_function("pipeline_full", |b| {
        b.iter(|| {
            fixture.compose();
            let measurements = fixture.measure();
            let layout_tree = measurements.layout_tree();
            let scene = renderer.render(&layout_tree);
            black_box(scene);
        });
    });
}

criterion_group!(
    pipeline,
    bench_composition,
    bench_measure,
    bench_layout,
    bench_render,
    bench_full_pipeline
);
criterion_main!(pipeline);
