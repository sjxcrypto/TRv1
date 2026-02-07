# TRv1 Genesis Configuration

> Reference document for all genesis parameters.  Values marked *adjustable*
> can be tuned before mainnet launch; all others are baked into the genesis
> block and cannot be changed without a hard fork.

---

## 1. Network Parameters

| Parameter | Value | Notes |
|-----------|-------|-------|
| **Ticks per slot** | 64 | Solana default; each tick ≈ 15.6 ms |
| **Slots per epoch** | 86 400 | 1 day at 1-second slot times |
| **Target slot time** | 1 second | Enforced by PoH + leader schedule |
| **Block time** | 1 second | 1 block = 1 slot |
| **Hashes per tick** | TBD | Calibrated per validator hardware at launch |

### Rationale

Daily epochs simplify reward distribution, slashing windows, and human
reasoning about network time.  86 400 slots × 1 s = exactly 24 hours.

---

## 2. Economic Parameters

### 2.1 Inflation / Staking

| Parameter | Value | Notes |
|-----------|-------|-------|
| **Validator staking rate** | 5% APY (flat) | No disinflationary curve — constant 5% annual yield for active validators |
| **Inflation source** | Newly minted tokens | Similar to Solana, but with a fixed rate instead of a declining schedule |

### 2.2 Fee Model — EIP-1559 Dynamic Base Fee

TRv1 replaces Solana's static fee model with an EIP-1559-style dynamic base
fee that adjusts per-slot based on demand.

| Parameter | Value | Notes |
|-----------|-------|-------|
| **Base fee adjustment** | ±12.5% per slot | When slot utilisation > 50%, base fee increases; when < 50%, it decreases |
| **Minimum base fee** | 5 000 lamports | Floor to prevent spam at zero cost |
| **Priority fee** | Optional, additive | Users can tip validators above the base fee |

### 2.3 Fee Split

Transaction fees (base + priority) are split as follows:

| Destination | Launch (epoch 0) | Maturity (epoch 1825) | Transition |
|-------------|:----------------:|:---------------------:|:----------:|
| **Burn** | 10% | 10% | Constant |
| **Validator** | 0% | 40% | Linear increase over 1 825 epochs |
| **Treasury** | 45% | 25% | Linear decrease over 1 825 epochs |
| **Developer** | 45% | 25% | Linear decrease over 1 825 epochs |

**Transition formula** (per epoch `e`, where `0 ≤ e ≤ 1825`):

```
progress     = min(e / 1825, 1.0)
burn_pct     = 10                         # constant
validator_pct = 0  + 40 × progress        # 0% → 40%
treasury_pct  = 45 - 20 × progress        # 45% → 25%
developer_pct = 45 - 20 × progress        # 45% → 25%
```

After epoch 1 825 (~5 years of daily epochs), the split is frozen at the
maturity values.

### 2.4 Fee Transition Timeline

| Epoch | ~Calendar | Burn | Validator | Treasury | Developer |
|------:|:---------:|:----:|:---------:|:--------:|:---------:|
| 0 | Day 1 | 10% | 0% | 45% | 45% |
| 365 | ~1 year | 10% | 8% | 41% | 41% |
| 730 | ~2 years | 10% | 16% | 37% | 37% |
| 1095 | ~3 years | 10% | 24% | 33% | 33% |
| 1460 | ~4 years | 10% | 32% | 29% | 29% |
| 1825 | ~5 years | 10% | 40% | 25% | 25% |

---

## 3. Validator Parameters

| Parameter | Value | Notes |
|-----------|-------|-------|
| **Active validator cap** | 200 | Top 200 by stake weight participate in consensus |
| **Standby validators** | Unlimited | Earn no rewards; ready to rotate in |
| **Jail threshold** | 86 400 slots (24 h) | Missing this many consecutive slots triggers jailing |
| **Jail duration** | 1 epoch (24 h) | Jailed validators sit out one full epoch |

### 3.1 Slashing Rates

Slashing is progressive — severity increases with repeated offences.

| Offence | Slash % of Stake | Condition |
|---------|:----------------:|-----------|
| First offence | 5% | Single safety violation or extended downtime |
| Second offence | 10% | Within 30 epochs of first |
| Third+ offence | 25% | Within 30 epochs of second |

Slashed tokens are **burned** (removed from supply).

---

## 4. Passive Staking Tiers

Non-validator token holders can earn yield by locking tokens.  Rewards are
expressed as a percentage of the validator staking rate (5% APY).

| Lock Period | % of Validator Rate | Approx APY | Governance Vote Weight |
|:-----------:|:-------------------:|:----------:|:----------------------:|
| No lock | 5% | 0.25% | 0× |
| 30 days | 10% | 0.50% | 0.10× |
| 90 days | 20% | 1.00% | 0.20× |
| 180 days | 30% | 1.50% | 0.30× |
| 360 days | 50% | 2.50% | 0.50× |
| Permanent | 120% | 6.00% | 1.50× |

### Early Unlock Penalties

| Lock Period | Penalty (% of principal) |
|:-----------:|:------------------------:|
| No lock | 0% |
| 30 days | 2.5% |
| 90 days | 5.0% |
| 180 days | 7.5% |
| 360 days | 12.5% |
| Permanent | **Cannot unlock** |

Penalties are **burned**.

---

## 5. Genesis Accounts

### 5.1 Treasury

| Field | Value |
|-------|-------|
| **Program ID** | `Treasury11111111111111111111111111111111111` |
| **Config account** | Derived at genesis (PDA or pre-allocated) |
| **Treasury token account** | Funded at genesis with initial allocation |
| **Initial authority** | 5-of-7 multisig |
| **Governance active** | `false` |
| **Initial balance** | TBD (depends on token distribution plan) |

### 5.2 Helper Node Pool

| Field | Value |
|-------|-------|
| **Program ID** | TBD |
| **Initial allocation** | 0% (reserved for future activation) |

Helper nodes are a planned feature for network utility services (RPC,
indexing, light clients, etc.).  The pool is allocated at genesis but
receives 0% of fees until activated by governance.

### 5.3 Initial Validator Stakes

The genesis block includes pre-staked accounts for the bootstrap validator
set.  Exact stake amounts depend on the token distribution plan.

| Parameter | Value |
|-----------|-------|
| **Genesis validator count** | 1 (bootstrap) |
| **Bootstrap validator stake** | TBD |
| **Additional validators** | Join via standard staking after genesis |

---

## 6. Token Supply

| Parameter | Value | Notes |
|-----------|-------|-------|
| **Total supply at genesis** | TBD | Minted in genesis block |
| **Inflation model** | 5% annual (flat) | All new tokens go to validator rewards |
| **Burn mechanisms** | 10% of fees + slashing + early unlock penalties | Deflationary pressure |

---

## 7. Program IDs (Built-in)

| Program | ID |
|---------|----|
| System | `11111111111111111111111111111111` |
| Vote | (Solana default) |
| Treasury | `Treasury11111111111111111111111111111111111` |
| Passive Stake | `Pass1veStake1111111111111111111111111111111` |
| BPF Loader | (Solana default) |

---

## 8. Open Questions

- [ ] Exact total token supply at genesis
- [ ] Initial treasury balance
- [ ] Bootstrap validator stake amount
- [ ] Hashes per tick (hardware-dependent calibration)
- [ ] Helper node pool program design and activation timeline
- [ ] Developer fee split mechanics (who qualifies as "developer"?)
- [ ] Multisig key holders for initial treasury authority
