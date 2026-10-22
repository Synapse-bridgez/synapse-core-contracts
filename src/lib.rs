#![no_std]
#![allow(clippy::too_many_arguments)]

#[cfg(test)]
extern crate std;

use soroban_sdk::{contract, contractimpl, Address, Bytes, Env, String, Symbol, Vec};

mod events;
mod storage;
mod types;

#[cfg(test)]
mod test_events_conformance;

use crate::types::{
    DataKey, Dispute, DisputeStatus, Evidence, Milestone, MilestoneStatus, Project, ProjectStatus,
    Reputation, Review,
};

#[contract]
pub struct Contract;

#[contractimpl]
impl Contract {
    /// Initialize the contract with an admin address.
    pub fn initialize(env: Env, admin: Address) {
        if storage::has_admin(&env) {
            panic!("already initialized");
        }
        storage::set_admin(&env, &admin);
        events::emit_initialized(&env, &admin);
    }

    /// Return the current admin address.
    pub fn get_admin(env: Env) -> Address {
        storage::get_admin(&env)
    }

    /// Create a new project owned by `owner`.
    pub fn create_project(
        env: Env,
        owner: Address,
        title: String,
        description: String,
        total_budget: i128,
    ) -> u64 {
        owner.require_auth();
        let id = storage::next_project_id(&env);
        let project = Project {
            id,
            owner: owner.clone(),
            title,
            description,
            total_budget,
            status: ProjectStatus::Open,
            created_at: env.ledger().timestamp(),
        };
        storage::set_project(&env, &project);
        events::emit_project_created(&env, &project);
        id
    }

    /// Fetch a project by id.
    pub fn get_project(env: Env, project_id: u64) -> Project {
        storage::get_project(&env, project_id)
    }

    /// Add a milestone to a project. Only the project owner may do this.
    pub fn add_milestone(
        env: Env,
        owner: Address,
        project_id: u64,
        title: String,
        amount: i128,
    ) -> u64 {
        owner.require_auth();
        let project = storage::get_project(&env, project_id);
        if project.owner != owner {
            panic!("not project owner");
        }
        let id = storage::next_milestone_id(&env);
        let milestone = Milestone {
            id,
            project_id,
            title,
            amount,
            status: MilestoneStatus::Pending,
        };
        storage::set_milestone(&env, &milestone);
        events::emit_milestone_added(&env, &milestone);
        id
    }

    /// Fetch a milestone by id.
    pub fn get_milestone(env: Env, milestone_id: u64) -> Milestone {
        storage::get_milestone(&env, milestone_id)
    }

    /// Mark a milestone as completed. Only the project owner may do this.
    pub fn complete_milestone(env: Env, owner: Address, milestone_id: u64) {
        owner.require_auth();
        let mut milestone = storage::get_milestone(&env, milestone_id);
        let project = storage::get_project(&env, milestone.project_id);
        if project.owner != owner {
            panic!("not project owner");
        }
        milestone.status = MilestoneStatus::Completed;
        storage::set_milestone(&env, &milestone);
        events::emit_milestone_completed(&env, &milestone);
    }

    /// Submit evidence for a milestone.
    pub fn submit_evidence(
        env: Env,
        submitter: Address,
        milestone_id: u64,
        uri: String,
        hash: Bytes,
    ) -> u64 {
        submitter.require_auth();
        let milestone = storage::get_milestone(&env, milestone_id);
        let id = storage::next_evidence_id(&env);
        let evidence = Evidence {
            id,
            milestone_id,
            submitter: submitter.clone(),
            uri,
            hash,
            submitted_at: env.ledger().timestamp(),
        };
        storage::set_evidence(&env, &evidence);
        events::emit_evidence_submitted(&env, &evidence, &milestone);
        id
    }

    /// Fetch evidence by id.
    pub fn get_evidence(env: Env, evidence_id: u64) -> Evidence {
        storage::get_evidence(&env, evidence_id)
    }

    /// Open a dispute on a project.
    pub fn open_dispute(
        env: Env,
        initiator: Address,
        project_id: u64,
        reason: String,
    ) -> u64 {
        initiator.require_auth();
        let project = storage::get_project(&env, project_id);
        let id = storage::next_dispute_id(&env);
        let dispute = Dispute {
            id,
            project_id,
            initiator: initiator.clone(),
            reason,
            status: DisputeStatus::Open,
            opened_at: env.ledger().timestamp(),
        };
        storage::set_dispute(&env, &dispute);
        events::emit_dispute_opened(&env, &dispute, &project);
        id
    }

    /// Fetch a dispute by id.
    pub fn get_dispute(env: Env, dispute_id: u64) -> Dispute {
        storage::get_dispute(&env, dispute_id)
    }

    /// Resolve an open dispute. Only the admin may do this.
    pub fn resolve_dispute(env: Env, admin: Address, dispute_id: u64, resolution: String) {
        admin.require_auth();
        let stored_admin = storage::get_admin(&env);
        if stored_admin != admin {
            panic!("not admin");
        }
        let mut dispute = storage::get_dispute(&env, dispute_id);
        dispute.status = DisputeStatus::Resolved;
        storage::set_dispute(&env, &dispute);
        events::emit_dispute_resolved(&env, &dispute, &resolution);
    }

    /// Submit a review for a completed project.
    pub fn submit_review(
        env: Env,
        reviewer: Address,
        project_id: u64,
        rating: u32,
        comment: String,
    ) -> u64 {
        reviewer.require_auth();
        let project = storage::get_project(&env, project_id);
        let id = storage::next_review_id(&env);
        let review = Review {
            id,
            project_id,
            reviewer: reviewer.clone(),
            rating,
            comment,
            created_at: env.ledger().timestamp(),
        };
        storage::set_review(&env, &review);
        events::emit_review_submitted(&env, &review, &project);
        id
    }

    /// Fetch a review by id.
    pub fn get_review(env: Env, review_id: u64) -> Review {
        storage::get_review(&env, review_id)
    }

    /// Fetch the reputation record for an address.
    pub fn get_reputation(env: Env, who: Address) -> Reputation {
        storage::get_reputation(&env, &who)
    }

    /// Update the reputation record for an address. Only the admin may do this.
    pub fn update_reputation(env: Env, admin: Address, who: Address, delta: i64) {
        admin.require_auth();
        let stored_admin = storage::get_admin(&env);
        if stored_admin != admin {
            panic!("not admin");
        }
        let mut reputation = storage::get_reputation(&env, &who);
        reputation.score = reputation.score.saturating_add(delta);
        storage::set_reputation(&env, &reputation);
        events::emit_reputation_updated(&env, &who, &reputation, delta);
    }

    /// Return the list of topics the contract emits.
    pub fn topics(env: Env) -> Vec<Symbol> {
        events::all_topics(&env)
    }
}