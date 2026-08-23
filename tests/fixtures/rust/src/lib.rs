pub struct Counter {
    value: u32,
}

impl Counter {
    pub fn increment(&mut self) {
        self.value += 1;
    }

    pub fn value(&self) -> u32 {
        self.value
    }
}

pub fn tick(counter: &mut Counter) {
    counter.increment();
}
