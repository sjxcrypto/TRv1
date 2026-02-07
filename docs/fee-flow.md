# TRv1 Fee Flow

> How transaction fees are calculated, split, and distributed to validators,
> the treasury, and dApp developers.

---

## 1. Fee Calculation (EIP-1559 Style)

TRv1 uses a two-component fee model inspired by Ethereum's EIP-1559:

| Component        | Description                                                                 |
|------------------|-----------------------------------------------------------------------------|
| **Base fee**     | Protocol-determined minimum per compute-unit cost. Adjusts dynamically based on network utilisation (increases when blocks are >50 % full, decreases otherwise). |
| **Priority fee** | Optional tip set by the user to incentivise validators to include the transaction sooner. |

```
total_fee = (base_fee + priority_fee) × compute_units_consumed
```

The **base fee** portion is subject to the four-way split described below.
The **priority fee** goes entirely to the block-producing validator.

---

## 2. Four-Way Fee Split

After the base fee is collected it is divided among four destinations:

| Destination        | Description                                                        |
|--------------------|--------------------------------------------------------------------|
| **Burn**           | Permanently removed from supply — deflationary pressure.           |
| **Validator**      | Rewarded to the leader who produced the block.                     |
| **Treasury**       | Protocol-controlled account for grants, development, ecosystem.    |
| **dApp Developer** | Shared among the programs the transaction invoked (this document). |

The exact percentages change over a 5-year transition (see §6).

---

## 3. Developer Share Attribution

### 3.1 Registration

Before a program can receive fees its deployer must register a
`ProgramRevenueConfig` by calling the **Developer Rewards program**'s
`RegisterRevenueRecipient` instruction.

* Only the program's **upgrade authority** may register.
* The config is stored as a PDA: `[b"program_revenue_config", program_id]`.
* A **7-day cooldown** begins at registration — no fees accrue until the
  cooldown expires (≈ 1 512 000 slots).

### 3.2 Per-Transaction Attribution

When a transaction is executed the runtime:

1. Identifies every program the transaction invoked (top-level and CPI).
2. Filters to programs that have a registered, active `ProgramRevenueConfig`
   **and** whose invocation consumed **> 1 000 compute units**.
3. Computes each qualifying program's share of the developer pool
   proportionally to its CU consumption:

```
program_share = developer_pool × (program_CU / total_qualifying_CU)
```

4. Credits the computed amount to each program's `unclaimed_fees` counter by
   invoking the internal `CreditDeveloperFees` instruction.

### 3.3 Multi-Program Transactions

When a single transaction invokes **multiple** qualifying programs the
developer share is split **pro-rata by compute-unit consumption**.

**Example:** A swap transaction uses two programs:

| Program  | CU consumed | Share of developer pool |
|----------|-------------|------------------------|
| DEX A    | 120 000     | 120 000 / 180 000 = 66.7 % |
| Oracle B |  60 000     |  60 000 / 180 000 = 33.3 % |

Programs that consumed ≤ 1 000 CU are excluded from the split entirely.

---

## 4. Multi-Recipient Revenue Splits

A program's `update_authority` may configure **up to 10 recipients** via the
`AddRevenueSplit` instruction. Each recipient is assigned a share in **basis
points** (1 bps = 0.01 %) and the shares must sum to exactly 10 000.

When fees are claimed (`ClaimDeveloperFees`), lamports are distributed
according to the configured split. The last recipient in the list receives the
remainder to avoid rounding dust.

If no revenue splits are configured, 100 % goes to the primary
`revenue_recipient`.

---

## 5. Anti-Gaming Measures

| Measure                        | Details                                                                                    |
|--------------------------------|--------------------------------------------------------------------------------------------|
| **Minimum CU threshold**       | A program invocation must consume **> 1 000 CU** for the transaction to qualify. Prevents trivial no-op programs from siphoning fees. |
| **7-day cooldown**             | Newly registered programs cannot earn fees for ≈ 7 days (1 512 000 slots at 400 ms/slot). Prevents rapid program churn / wash-trading. |
| **10 % per-epoch cap**         | No single program may receive more than **10 %** of total developer fees in any epoch. Prevents a single popular (or self-dealing) dApp from monopolising the pool. |
| **Upgrade-authority gating**   | Only the program's upgrade authority can register or modify revenue configs. Prevents third parties from hijacking fee streams. |
| **CPI attribution by CU**     | Share is proportional to actual compute consumed, not number of invocations. Prevents CPI-spam attacks where a program calls itself many times with minimal work. |

---

## 6. Five-Year Transition Schedule

The fee split transitions linearly from **Launch** to **Maturity** over
≈ 912 epochs (roughly 5 years at ~2-day epochs).

| Year | Burn  | Validator | Treasury | Developer |
|------|-------|-----------|----------|-----------|
| 0    | 10 %  |  0 %      | 45 %     | 45 %      |
| 1    | 13 %  |  5 %      | 41 %     | 41 %      |
| 2    | 16 %  | 10 %      | 37 %     | 37 %      |
| 3    | 19 %  | 15 %      | 33 %     | 33 %      |
| 4    | 22 %  | 20 %      | 29 %     | 29 %      |
| 5+   | 25 %  | 25 %      | 25 %     | 25 %      |

### Interpolation formula

For a given epoch `e` (0-indexed from genesis):

```
progress = min(e / TRANSITION_EPOCHS, 1.0)

burn_bps      = 1000 + progress × (2500 - 1000)
validator_bps =    0 + progress × (2500 -    0)
treasury_bps  = 4500 - progress × (4500 - 2500)
developer_bps = 4500 - progress × (4500 - 2500)
```

All values are in basis points (10 000 = 100 %).

---

## 7. Claiming Fees

Accumulated fees are stored in each program's `ProgramRevenueConfig.unclaimed_fees`.
Anyone may call `ClaimDeveloperFees` (it is permissionless) — funds always
transfer to the registered recipient(s), never to the caller.

Funds are sourced from the **Developer Fee Pool** PDA
(`[b"developer_fee_pool"]`), which the runtime tops up as part of the
per-transaction fee distribution.

---

## 8. Account Layout

### ProgramRevenueConfig (PDA)

| Field               | Type           | Description                                |
|---------------------|----------------|--------------------------------------------|
| `version`           | `u8`           | Schema version (currently 1)               |
| `program_id`        | `Pubkey`       | Program this config belongs to             |
| `revenue_recipient` | `Pubkey`       | Primary fee recipient                      |
| `update_authority`  | `Pubkey`       | Can modify config (initially upgrade auth) |
| `is_active`         | `bool`         | Whether fees are being credited            |
| `revenue_splits`    | `Vec<Split>`   | Up to 10 split entries (bps)               |
| `total_fees_earned` | `u64`          | Lifetime lamports earned                   |
| `epoch_fees_earned` | `u64`          | Lamports earned this epoch                 |
| `last_epoch`        | `u64`          | Epoch of last roll-over                    |
| `eligible_after_slot` | `u64`        | Slot when cooldown expires                 |
| `unclaimed_fees`    | `u64`          | Lamports available to claim                |

### EpochFeeTracker (Singleton PDA)

| Field                   | Type  | Description                         |
|-------------------------|-------|-------------------------------------|
| `version`               | `u8`  | Schema version                      |
| `epoch`                 | `u64` | Current epoch                       |
| `total_developer_fees`  | `u64` | Total dev fees credited this epoch  |

---

## 9. Diagram

```
┌──────────────┐
│  Transaction │
│  (user pays) │
└──────┬───────┘
       │ total_fee = (base + priority) × CU
       ▼
┌──────────────────────────────────────────┐
│            Fee Collector                 │
│                                          │
│  priority_fee ──────────► Validator      │
│                                          │
│  base_fee is split:                      │
│    ├─ burn_bps%     ──► 🔥 Burn          │
│    ├─ validator_bps% ──► Validator        │
│    ├─ treasury_bps%  ──► Treasury         │
│    └─ developer_bps% ──► Dev Fee Pool     │
└──────────────────────────────────────────┘
                                  │
                                  ▼
                  ┌───────────────────────────┐
                  │    Developer Fee Pool     │
                  │ (PDA: developer_fee_pool) │
                  └────────────┬──────────────┘
                               │
            ┌──────────────────┼──────────────────┐
            ▼                  ▼                   ▼
     ┌─────────────┐   ┌─────────────┐   ┌──────────────┐
     │  Program A  │   │  Program B  │   │  Program C   │
     │ (pro-rata   │   │ (pro-rata   │   │ (no config   │
     │  by CU)     │   │  by CU)     │   │  → skipped)  │
     └──────┬──────┘   └──────┬──────┘   └──────────────┘
            │                 │
            ▼                 ▼
       unclaimed_fees    unclaimed_fees
            │                 │
     ClaimDeveloperFees  ClaimDeveloperFees
            │                 │
            ▼                 ▼
    ┌───────────────┐  ┌──────────────┐
    │  Recipient(s) │  │ Recipient(s) │
    │  (via splits) │  │ (via splits) │
    └───────────────┘  └──────────────┘
```
