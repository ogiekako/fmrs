use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
pub struct JsonResponse {
    pub(super) solutions: u16,
    pub(super) jkf: String,
}

#[wasm_bindgen]
impl JsonResponse {
    pub fn solutions(&self) -> u16 {
        self.solutions
    }
    pub fn jkf(self) -> String {
        self.jkf
    }
}
