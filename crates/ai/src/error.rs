//! Errors of the AI backend. The texts are neutral on purpose: they never name the tool that
//! prepares the document, because the user doesn't need to know (or be told) how it was done.

use crate::Provider;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiError {
    /// The local component that prepares the document is not installed.
    PreparerMissing,
    /// The local component ran but failed, or its files could not be handled.
    PrepareFailed,
    /// The prepared document has no readable text (a scan without OCR).
    NoText,
    /// The agent command of this provider was not found.
    AgentMissing(Provider),
    /// The agent started but failed; carries what it reported (stderr tail, or a short reason).
    AgentFailed(String),
    /// The caller cancelled the request.
    Cancelled,
    /// The model identifier is not acceptable.
    InvalidModel,
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PreparerMissing => f.write_str("Falta un componente local para preparar el documento; consulta la documentación."),
            Self::PrepareFailed => f.write_str("No se pudo preparar el documento en este equipo."),
            Self::NoText => f.write_str("El documento no contiene texto legible; aplica OCR primero."),
            Self::AgentMissing(p) => write!(f, "{} no está instalado o no aparece en PATH.", p.id()),
            Self::AgentFailed(reason) => f.write_str(reason),
            Self::Cancelled => f.write_str("Operación cancelada."),
            Self::InvalidModel => f.write_str("El identificador de modelo no es válido."),
        }
    }
}

impl std::error::Error for AiError {}
