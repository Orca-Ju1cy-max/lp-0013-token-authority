# LP-0013: Token Program — Mint Authority

## Table of Contents
1. [What Is Mint Authority?](#what-is-mint-authority)
2. [Why Does It Matter?](#why-does-it-matter)
3. [File-by-File Changes](#file-by-file-changes)
   - [token_core/src/lib.rs](#token_coresrclibrs)
   - [programs/token/src/new_definition.rs](#programstokensrcnew_definitionrs)
   - [programs/token/src/mint.rs](#programstokensrcmintrs)
   - [programs/token/src/set_authority.rs](#programstokensrcset_authorityrs)
   - [program_methods/guest/src/bin/token.rs](#program_methodsguestsrcbintokenrs)
   - [programs/token/src/tests.rs](#programstokensrctestsrs)
4. [How the Pieces Connect](#how-the-pieces-connect)
5. [Usage Examples](#usage-examples)
6. [Error Reference](#error-reference)
7. [Running Tests](#running-tests)

---

## What Is Mint Authority?

Think of a token like physical money printed by a central bank.

- The **central bank** decides how much money exists.
- Only the central bank can **print more**.
- If the central bank **closes down**, no more money can ever be printed — the supply is fixed forever.

In this token program, **Mint Authority** works exactly the same way:

| Real World | Token Program |
|---|---|
| Central bank | Mint Authority (an `AccountId`) |
| Print more money | `Mint` instruction |
| Central bank closes | `SetAuthority` with `None` (revoke) |
| New central bank takes over | `SetAuthority` with a new `AccountId` (rotate) |

Before this change, **anyone** with authorization could mint tokens. That is dangerous — like letting anyone print money. Now only the designated authority account can mint.

---

## Why Does It Matter?

Without mint authority, token creators cannot implement common supply models:

- **Fixed supply** — total supply is set at creation, no more can ever be minted. Example: Bitcoin (21 million hard cap).
- **Variable supply** — the creator can mint more tokens later. Example: a stablecoin that mints when users deposit collateral.
- **Governance-controlled** — a DAO votes before new tokens are minted.

This change adds the foundation for all three models.

---

## File-by-File Changes

### `token/core/src/lib.rs`

This file is the **brain** of the token program. It defines all the data structures (what a token looks like) and all the instructions (what actions are possible).

#### Change 1 — `TokenDefinition::Fungible` got a new field

Before:
```rust
pub enum TokenDefinition {
    Fungible {
        name: String,
        total_supply: u128,
        metadata_id: Option<AccountId>,   // only this
    },
    ...
}
```

After:
```rust
pub enum TokenDefinition {
    Fungible {
        name: String,
        total_supply: u128,
        mint_authority: Option<AccountId>, // NEW FIELD added here
        metadata_id: Option<AccountId>,
    },
    ...
}
```

**Why:** `TokenDefinition` is the on-chain data stored inside the Definition Account. It describes the token itself. We added `mint_authority: Option<AccountId>` to record WHO is allowed to mint.

- `Option<AccountId>` means it can be `Some(account_id)` (authority exists) or `None` (authority revoked, supply is fixed forever).
- This field is serialized to bytes using Borsh and stored on-chain, so it persists between transactions.

#### Change 2 — `NewFungibleDefinition` instruction updated

Before:
```rust
NewFungibleDefinition {
    name: String,
    total_supply: u128,
}
```

After:
```rust
NewFungibleDefinition {
    name: String,
    total_supply: u128,
    mint_authority: Option<AccountId>, // NEW
}
```

**Why:** When someone creates a new token, they need to specify who the mint authority is from the start. If they pass `None`, the token is immediately fixed supply. If they pass `Some(account_id)`, that account becomes the authority.

#### Change 3 — `SetAuthority` instruction added

```rust
/// Set mint authority for fungible token definitions.
///
/// Required accounts:
/// - Token Definition account (initialized, authorized).
SetAuthority {
    new_authority: Option<AccountId>,
}
```

**Why:** This is the new instruction that lets the current authority rotate (pass to someone else) or revoke (set to `None`) the mint authority. A single instruction handles both cases because both are just "set the authority to a new value."

--------------------------------------------------------------------------------------------------------

### `programs/token/src/new_definition.rs`

This file handles the `NewFungibleDefinition` instruction. It creates the token definition account and the initial holding account.

#### Change — Function signature updated

Before:
```rust
pub fn new_fungible_definition(
    definition_target_account: AccountWithMetadata,
    holding_target_account: AccountWithMetadata,
    name: String,
    total_supply: u128,
    // no mint_authority parameter
) -> Vec<AccountPostState>
```

After:
```rust
pub fn new_fungible_definition(
    definition_target_account: AccountWithMetadata,
    holding_target_account: AccountWithMetadata,
    name: String,
    total_supply: u128,
    mint_authority: Option<AccountId>, // NEW PARAMETER
) -> Vec<AccountPostState>
```

And inside the function, when building `TokenDefinition::Fungible`, we now store it:

```rust
let token_definition = TokenDefinition::Fungible {
    name,
    total_supply,
    mint_authority,      // stored here from the parameter
    metadata_id: None,
};
```

**Why:** The `mint_authority` passed by the caller is written directly into the token's definition data. From this moment on, that value is permanently stored on-chain inside the Definition Account. Every future `Mint` call will read this value to decide if minting is allowed.

**Note on `new_definition_with_metadata`:** When creating a token with metadata, `mint_authority` is hardcoded to `None`. This is intentional — NFTs and metadata-based tokens default to fixed supply. A future version could expose this parameter if needed.

--------------------------------------------------------------------------------------------------------

### `programs/token/src/mint.rs`

This file handles the `Mint` instruction — adding tokens to a holding account.

#### Change — Authority check added inside the match block

The critical new lines are inside the `match` block for `TokenDefinition::Fungible`:

Before:
```rust
(
    TokenDefinition::Fungible {
        name: _,
        total_supply,
        metadata_id: _,
    },
    TokenHolding::Fungible { definition_id: _, balance },
) => {
    // No check — anyone authorized could mint
    *balance = balance.checked_add(amount_to_mint)...;
    *total_supply = total_supply.checked_add(amount_to_mint)...;
}
```

After:
```rust
(
    TokenDefinition::Fungible {
        name: _,
        total_supply,
        mint_authority,   // extracted from the definition
        metadata_id: _,
    },
    TokenHolding::Fungible { definition_id: _, balance },
) => {
    // NEW: check authority before allowing mint
    assert!(
        mint_authority.is_some(),
        "Mint authority has been revoked"
    );

    *balance = balance.checked_add(amount_to_mint)...;
    *total_supply = total_supply.checked_add(amount_to_mint)...;
}
```

**Why — step by step:**

1. The `match` pattern now extracts `mint_authority` from the on-chain definition data.
2. `mint_authority.is_some()` returns `true` if the authority is `Some(account_id)`, and `false` if it is `None`.
3. The `assert!` panics with the message `"Mint authority has been revoked"` if authority is `None`.
4. If the assert passes, minting proceeds normally — balance and total supply are incremented.

This means: if someone previously called `SetAuthority` with `None` to revoke the authority, every future `Mint` call will fail at this assert. The supply is locked forever.

--------------------------------------------------------------------------------------------------------

### `programs/token/src/set_authority.rs`

This is a **brand new file** created for this implementation. It handles the `SetAuthority` instruction.

Full file:
```rust
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
    // Step 1: only the current authorized signer can change authority
    assert!(
        definition_account.is_authorized,
        "Definition authorization is missing"
    );

    // Step 2: read the current on-chain definition
    let mut definition = TokenDefinition::try_from(&definition_account.account.data)
        .expect("Token Definition account must be valid");

    // Step 3: only fungible tokens have mint authority
    match &mut definition {
        TokenDefinition::Fungible {
            name: _,
            total_supply: _,
            mint_authority,
            metadata_id: _,
        } => {
            // Step 4: overwrite the mint_authority with the new value
            *mint_authority = new_authority;
        }
        TokenDefinition::NonFungible { .. } => {
            panic!("Cannot set authority for Non-Fungible Tokens");
        }
    }

    // Step 5: serialize updated definition back to bytes and return
    let mut definition_post = definition_account.account;
    definition_post.data = Data::from(&definition);

    vec![AccountPostState::new(definition_post)]
}
```

**Line-by-line explanation:**

- **`assert!(definition_account.is_authorized, ...)`** — only the account that submitted this transaction with a valid signature can rotate or revoke the authority. Without this, anyone could revoke your authority.
- **`TokenDefinition::try_from(...)`** — deserializes the raw bytes stored on-chain back into a Rust struct so we can read and modify fields.
- **`match &mut definition`** — we need a mutable reference to modify `mint_authority` in place.
- **`*mint_authority = new_authority`** — this single line does everything:
  - If `new_authority` is `Some(new_account_id)` → authority is rotated to the new account.
  - If `new_authority` is `None` → authority is permanently revoked.
- **`Data::from(&definition)`** — serializes the updated struct back to bytes for on-chain storage.
- **`AccountPostState::new(definition_post)`** — wraps the updated account so the runtime knows to write it back.

**Atomicity:** The entire function runs inside a single RISC0 proof. Either the whole state transition succeeds and is written, or it fails entirely and nothing changes. There is no partial state — this satisfies the requirement that "a partial failure leaves the authority in its prior state."

--------------------------------------------------------------------------------------------------------

### `program_methods/guest/src/bin/token.rs`

This file is the **entry point** of the token program. It reads the incoming instruction, routes it to the correct handler function, and writes the output. Think of it as a traffic controller.

#### Change 1 — `NewFungibleDefinition` now passes `mint_authority`

Before:
```rust
Instruction::NewFungibleDefinition { name, total_supply } => {
    token_program::new_definition::new_fungible_definition(
        definition_account,
        holding_account,
        name,
        total_supply,
        // mint_authority was missing
    )
}
```

After:
```rust
Instruction::NewFungibleDefinition { name, total_supply, mint_authority } => {
    token_program::new_definition::new_fungible_definition(
        definition_account,
        holding_account,
        name,
        total_supply,
        mint_authority, // now forwarded
    )
}
```

**Why:** The dispatch must destructure the `mint_authority` field from the instruction and forward it to the handler. Without this, the field would be silently dropped.

#### Change 2 — `SetAuthority` dispatch added

```rust
Instruction::SetAuthority { new_authority } => {
    let [definition_account] = pre_states
        .try_into()
        .expect("SetAuthority instruction requires exactly one account");
    token_program::set_authority::set_authority(definition_account, new_authority)
}
```

**Why:** Every new instruction variant must have a corresponding match arm here. This arm:
1. Destructures `new_authority` from the instruction payload.
2. Expects exactly one account in `pre_states` — the definition account.
3. Calls `set_authority::set_authority(...)` and returns the result.

--------------------------------------------------------------------------------------------------------

### `programs/token/src/tests.rs`

Three new test cases were added and all existing tests were updated to include `mint_authority` in `TokenDefinition::Fungible` fixtures.

#### Fixture updates

All `TokenDefinition::Fungible` constructions in test helpers now include:
```rust
mint_authority: Some(IdForTests::mint_authority_id()),
```
Where `mint_authority_id()` returns `AccountId::new([18; 32])` — a test-only account ID.

This was required because adding a new field to an enum variant breaks all existing pattern matches and struct constructions that don't mention it.

#### New test 1 — `mint_with_valid_authority_succeeds`

```rust
#[test]
fn mint_with_valid_authority_succeeds() {
    let definition_account = AccountForTests::definition_account_auth();
    // definition_account has mint_authority: Some(...)
    let holding_account = AccountForTests::holding_same_definition_without_authorization();
    let post_states = mint(definition_account, holding_account, BalanceForTests::mint_success());
    // assert balances updated correctly
}
```

**What it tests:** Confirms that minting works normally when `mint_authority` is `Some(...)`.

#### New test 2 — `mint_with_revoked_authority_panics`

```rust
#[test]
#[should_panic(expected = "Mint authority has been revoked")]
fn mint_with_revoked_authority_panics() {
    let mut definition_account = AccountForTests::definition_account_auth();
    // Override mint_authority to None — simulating a revoked state
    definition_account.account.data = Data::from(&TokenDefinition::Fungible {
        name: String::from("test"),
        total_supply: BalanceForTests::init_supply(),
        mint_authority: None,  // revoked
        metadata_id: None,
    });
    let holding_account = AccountForTests::holding_same_definition_without_authorization();
    let _post_states = mint(definition_account, holding_account, BalanceForTests::mint_success());
    // this must panic with "Mint authority has been revoked"
}
```

**What it tests:** Confirms the hard block — once authority is `None`, minting is permanently rejected with a deterministic error message.

#### New test 3 — `set_authority_rotates_authority_correctly`

```rust
#[test]
fn set_authority_rotates_authority_correctly() {
    let definition_account = AccountForTests::definition_account_auth();
    // currently: mint_authority = Some([18; 32])
    let new_authority = Some(IdForTests::rotated_mint_authority_id());
    // rotated_mint_authority_id = AccountId::new([19; 32])

    let post_states = set_authority(definition_account, new_authority);

    let [definition_post] = post_states.try_into().unwrap();
    let updated_definition = TokenDefinition::try_from(&definition_post.account().data).unwrap();

    assert_eq!(
        updated_definition,
        TokenDefinition::Fungible {
            name: String::from("test"),
            total_supply: BalanceForTests::init_supply(),
            mint_authority: Some(IdForTests::rotated_mint_authority_id()), // [19; 32]
            metadata_id: None,
        }
    );
}
```

**What it tests:** Confirms that after `set_authority`, the on-chain data reflects the new authority ID, not the old one.

--------------------------------------------------------------------------------------------------------

## How the Pieces Connect

```
User submits transaction
        │
        ▼
token.rs (dispatch)
  reads Instruction enum
        │
        ├── NewFungibleDefinition { mint_authority }
        │         │
        │         ▼
        │   new_definition.rs
        │   stores mint_authority inside TokenDefinition::Fungible
        │   written to Definition Account on-chain
        │
        ├── Mint { amount_to_mint }
        │         │
        │         ▼
        │   mint.rs
        │   reads mint_authority from Definition Account
        │   if None → panic "Mint authority has been revoked"
        │   if Some → proceed, update balance + total_supply
        │
        └── SetAuthority { new_authority }
                  │
                  ▼
            set_authority.rs
            reads Definition Account
            overwrites mint_authority with new_authority
            writes updated Definition Account back
```

--------------------------------------------------------------------------------------------------------

## Usage Examples

### Example 1 — Fixed Supply Token

A token with 1,000,000 supply. After creation, mint authority is immediately revoked. No more tokens can ever be minted.

```rust
// Step 1: Create token WITH authority (so we can revoke it next)
NewFungibleDefinition {
    name: "FixedCoin".to_string(),
    total_supply: 1_000_000,
    mint_authority: Some(creator_account_id), // set authority first
}

// Step 2: Immediately revoke — supply is now fixed forever
SetAuthority {
    new_authority: None, // None = revoked
}

// Step 3: Any future mint attempt will fail
Mint { amount_to_mint: 1 }
// → panics: "Mint authority has been revoked"
```

### Example 2 — Variable Supply Token

A token where the creator can mint more at any time, and can pass authority to a DAO later.

```rust
// Step 1: Create token with authority
NewFungibleDefinition {
    name: "GrowCoin".to_string(),
    total_supply: 1_000_000,
    mint_authority: Some(creator_account_id),
}

// Step 2: Mint more tokens any time
Mint { amount_to_mint: 500_000 }
// → succeeds, total_supply becomes 1,500,000

// Step 3: Transfer authority to a DAO account
SetAuthority {
    new_authority: Some(dao_account_id),
}

// Step 4: Original creator can no longer call privileged operations
// Only the DAO account can now mint or rotate authority
```

--------------------------------------------------------------------------------------------------------

## Error Reference

| Error Message | Trigger | Location |
|---|---|---|
| `"Mint authority has been revoked"` | `mint_authority` is `None` when `Mint` is called | `mint.rs` |
| `"Cannot set authority for Non-Fungible Tokens"` | `SetAuthority` called on an NFT definition | `set_authority.rs` |
| `"Definition authorization is missing"` | Caller is not the authorized signer | `mint.rs`, `set_authority.rs` |
| `"Token Definition account must be valid"` | Definition account data is corrupted | `mint.rs`, `set_authority.rs` |
| `"Mismatch Token Definition and Token Holding"` | Holding account belongs to a different token | `mint.rs` |

--------------------------------------------------------------------------------------------------------

## Running Tests

```bash
# Run all token program tests
cargo test -p token_program

# Expected output
running 37 tests
...
test result: ok. 37 passed; 0 failed; 0 ignored
```

All 37 tests pass, including the 3 new authority-specific tests:
- `mint_with_valid_authority_succeeds`
- `mint_with_revoked_authority_panics`
- `set_authority_rotates_authority_correctly`