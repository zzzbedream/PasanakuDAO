//! Contributions arrive as NEP-141 `ft_transfer_call` into this contract.
//! The message names the circle and, optionally, the member being paid for
//! (needed when a bridge or the treasurer agent pays on a member's behalf).

use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;
use near_sdk::json_types::U128;
use near_sdk::serde_json::json;
use near_sdk::{AccountId, PromiseOrValue, env, near, require};

use crate::events;
use crate::model::*;
use crate::{Contract, ContractExt};

#[near]
impl FungibleTokenReceiver for Contract {
    /// Credit one contribution. Returns the unused amount so the token
    /// contract refunds anything above the fixed contribution.
    fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        let parsed: ContributionMsg = near_sdk::serde_json::from_str(&msg).unwrap_or_else(|_| {
            env::panic_str("msg must be JSON {\"circle_id\",\"beneficiary\"?}")
        });
        let beneficiary = parsed.beneficiary.unwrap_or_else(|| sender_id.clone());
        let circle = self.circle_or_panic(&parsed.circle_id);
        require!(
            env::predecessor_account_id() == circle.token,
            "Wrong token contract"
        );
        require!(
            circle.status == CircleStatus::Active,
            "Circle is not active"
        );
        require!(
            circle.is_member(&beneficiary),
            "Beneficiary is not a member"
        );
        let contribution = circle.contribution.0;
        require!(
            amount.0 >= contribution,
            "Amount below the fixed contribution"
        );
        let key = circle.round_key(circle.current_round);
        let round_index = circle.current_round;
        let circle_id = circle.id.clone();

        let round = self
            .rounds
            .get_mut(&key)
            .unwrap_or_else(|| env::panic_str("Round not found"));
        require!(
            round.status == RoundStatus::Collecting,
            "Round is not collecting"
        );
        require!(
            !round.paid.contains(&beneficiary),
            "Member already paid this round"
        );
        round.paid.push(beneficiary.clone());
        round.pot = U128(round.pot.0 + contribution);

        events::emit(
            "contribution_received",
            json!({ "circle_id": circle_id, "round": round_index, "member": beneficiary,
                    "payer": sender_id, "amount": U128(contribution) }),
        );
        PromiseOrValue::Value(U128(amount.0 - contribution))
    }
}
