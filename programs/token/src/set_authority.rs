use nssa_core::{
    account::{AccountId, AccountWithMetadata, Data},
    program::AccountPostState,
};
use token_core::TokenDefinition;

#[must_use]
pub fn set_authority(
    definition_account: AccountWithMetadata,
    new_authority: Option<AccountId>,
) -> Vec<AccountPostState> {
    assert!(
        definition_account.is_authorized,
        "Definition authorization is missing"
    );

    let mut definition = TokenDefinition::try_from(&definition_account.account.data)
        .expect("Token Definition account must be valid");

    match &mut definition {
        TokenDefinition::Fungible {
            name: _,
            total_supply: _,
            mint_authority,
            metadata_id: _,
        } => {
            *mint_authority = new_authority;
        }
        TokenDefinition::NonFungible { .. } => {
            panic!("Cannot set authority for Non-Fungible Tokens");
        }
    }

    let mut definition_post = definition_account.account;
    definition_post.data = Data::from(&definition);

    vec![AccountPostState::new(definition_post)]
}
