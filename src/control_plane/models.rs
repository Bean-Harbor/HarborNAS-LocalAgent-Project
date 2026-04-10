//! Model registry entries for LLM/VLM/OCR/ASR providers.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Llm,
    Vlm,
    Ocr,
    Asr,
    Detector,
    Embedder,
}
