use compose_core::{Applier, Ix, Node};

pub struct SkiaApplier {
    nodes: Vec<Box<dyn Node>>,
}

impl Default for SkiaApplier {
    fn default() -> Self {
        Self::new()
    }
}

impl SkiaApplier {
    pub fn new() -> Self {
        Self { nodes: vec![] }
    }

    pub fn draw(&mut self) {
        println!("--- Drawing frame ---");
        for (ix, node) in self.nodes.iter().enumerate() {
            println!("  Node {}: {:?}", ix, node);
        }
    }
}

impl Applier for SkiaApplier {
    fn create<N: Node>(&mut self, init: N) -> Ix {
        let ix = self.nodes.len() as Ix;
        self.nodes.push(Box::new(init));
        ix
    }

    fn get_mut(&mut self, ix: Ix) -> &mut dyn Node {
        &mut *self.nodes[ix as usize]
    }

    fn remove(&mut self, ix: Ix) {
        self.nodes.remove(ix as usize);
    }
}