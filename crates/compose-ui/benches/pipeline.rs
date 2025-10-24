use compose_core::{location_key, Composition, Key, MemoryApplier};
use compose_ui::{
    composable, measure_layout, Column, ColumnSpec, HeadlessRenderer, LayoutMeasurements, Modifier,
    Row, RowSpec, Size, Text,
};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

const SECTION_COUNT: usize = 4;
const ROWS_PER_SECTION: usize = 32;
const MEASURE_ROWS_PER_SECTION_SAMPLES: &[usize] = &[8, 16, 24, 32, 40, 48, 56, 64];
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
    rows_per_section: usize,
    root_size: Size,
}

impl PipelineFixture {
    fn new(sections: usize, rows_per_section: usize, root_size: Size) -> Self {
        let key = location_key(file!(), line!(), column!());
        Self {
            composition: Composition::new(MemoryApplier::new()),
            key,
            sections,
            rows_per_section,
            root_size,
        }
    }

    fn compose(&mut self) {
        let sections = self.sections;
        let rows_per_section = self.rows_per_section;
        self.composition
            .render(self.key, || pipeline_content(sections, rows_per_section))
            .expect("composition");
    }

    fn measure(&mut self) -> LayoutMeasurements {
        let root = self.composition.root().expect("composition root");
        measure_layout(self.composition.applier_mut(), root, self.root_size).expect("measure")
    }
}

fn ui_object_count(sections: usize, rows_per_section: usize) -> usize {
    1 + sections * (2 + rows_per_section * 3)
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
    let mut group = c.benchmark_group("pipeline_measure");
    for &rows_per_section in MEASURE_ROWS_PER_SECTION_SAMPLES {
        let sections = SECTION_COUNT;
        let total_ui_objects = ui_object_count(sections, rows_per_section);
        group.bench_with_input(
            BenchmarkId::new("ui_objects", total_ui_objects),
            &(sections, rows_per_section),
            |b, &(sections, rows_per_section)| {
                let mut fixture = PipelineFixture::new(sections, rows_per_section, ROOT_SIZE);
                fixture.compose();

                b.iter(|| {
                    let measurements = fixture.measure();
                    black_box(measurements);
                });
            },
        );
    }
    group.finish();
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
