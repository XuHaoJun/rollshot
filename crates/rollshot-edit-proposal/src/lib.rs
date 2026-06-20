//! Visual edit-proposal foundation (spec §6.3): the review model that lowers to
//! `rollshot_image_document::EditOp`. No agent/LLM, UI, or capture code.

mod proposal;
mod review;

pub use proposal::{
    CandidateId, ConfidenceSummary, EditProposal, ProposalId, ProposedCandidate, ProposedEdit,
    Provenance, ProvenanceSource,
};

pub use review::{lower, ReviewDecision};
