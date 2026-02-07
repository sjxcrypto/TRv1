# TRv1 RPC API Reference

> **Namespace prefix**: All TRv1-specific methods use the `trv1_` prefix to distinguish them from upstream Solana/Agave RPC methods.

## Endpoint Availability

| Availability | Description |
|---|---|
| **All nodes** | Available on every node type (validator, RPC, archive) |
| **RPC nodes only** | Requires `--full-rpc-api`; may perform account scans |

---

## Table of Contents

1. [Passive Staking](#1-passive-staking)
2. [Fee Market](#2-fee-market)
3. [Validators](#3-validators)
4. [Treasury](#4-treasury)
5. [Governance](#5-governance)
6. [Developer Rewards](#6-developer-rewards)
7. [Network Info](#7-network-info)

---

## 1. Passive Staking

### `trv1_getPassiveStakeAccount`

Returns the details of a single passive-stake account.

**Availability:** All nodes

**Parameters:**

| # | Type | Description |
|---|------|-------------|
| 1 | `string` | Pubkey of the passive-stake account (base-58) |

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_getPassiveStakeAccount",
  "params": ["5ZWj7a1f8tWkjBESHKgrLmXshuXxqeY9SYcfbshpAqPG"]
}
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "pubkey": "5ZWj7a1f8tWkjBESHKgrLmXshuXxqeY9SYcfbshpAqPG",
    "owner": "9aE476sH92Vz7DMPyq5WLPkrKWivxeuTKEFKd2sZZcde",
    "stakedLamports": 1000000000,
    "tier": 2,
    "tierName": "90-day",
    "activatedAt": 1700000000,
    "lockupExpiresAt": 1707776000,
    "isWithdrawable": false,
    "totalRewardsEarned": 12500000,
    "lastRewardEpoch": 450,
    "currentApyBps": 650
  },
  "id": 1
}
```

---

### `trv1_getPassiveStakesByOwner`

Returns all passive-stake accounts owned by the given wallet.

**Availability:** RPC nodes only (performs account scan)

**Parameters:**

| # | Type | Description |
|---|------|-------------|
| 1 | `string` | Owner wallet pubkey (base-58) |

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_getPassiveStakesByOwner",
  "params": ["9aE476sH92Vz7DMPyq5WLPkrKWivxeuTKEFKd2sZZcde"]
}
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "pubkey": "5ZWj7a1f8tWkjBESHKgrLmXshuXxqeY9SYcfbshpAqPG",
      "owner": "9aE476sH92Vz7DMPyq5WLPkrKWivxeuTKEFKd2sZZcde",
      "stakedLamports": 1000000000,
      "tier": 2,
      "tierName": "90-day",
      "activatedAt": 1700000000,
      "lockupExpiresAt": 1707776000,
      "isWithdrawable": false,
      "totalRewardsEarned": 12500000,
      "lastRewardEpoch": 450,
      "currentApyBps": 650
    },
    {
      "pubkey": "3Kf1QxB7rKzS4McFjBGg7VKdwLDHp5chDRKpbhqy9wFp",
      "owner": "9aE476sH92Vz7DMPyq5WLPkrKWivxeuTKEFKd2sZZcde",
      "stakedLamports": 500000000,
      "tier": 0,
      "tierName": "flexible",
      "activatedAt": 1701000000,
      "lockupExpiresAt": 0,
      "isWithdrawable": true,
      "totalRewardsEarned": 3000000,
      "lastRewardEpoch": 450,
      "currentApyBps": 250
    }
  ],
  "id": 1
}
```

---

### `trv1_getPassiveStakingRates`

Returns the current passive-staking APY for each tier along with aggregate statistics.

**Availability:** All nodes

**Parameters:** None

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_getPassiveStakingRates",
  "params": []
}
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "epoch": 450,
    "tiers": [
      {
        "tier": 0,
        "tierName": "flexible",
        "lockupDays": 0,
        "apyBps": 250,
        "totalStakedLamports": 50000000000000,
        "accountCount": 12000
      },
      {
        "tier": 1,
        "tierName": "30-day",
        "lockupDays": 30,
        "apyBps": 450,
        "totalStakedLamports": 80000000000000,
        "accountCount": 8500
      },
      {
        "tier": 2,
        "tierName": "90-day",
        "lockupDays": 90,
        "apyBps": 650,
        "totalStakedLamports": 120000000000000,
        "accountCount": 6200
      },
      {
        "tier": 3,
        "tierName": "180-day",
        "lockupDays": 180,
        "apyBps": 850,
        "totalStakedLamports": 200000000000000,
        "accountCount": 3100
      }
    ],
    "totalStakedLamports": 450000000000000,
    "totalAccounts": 29800
  },
  "id": 1
}
```

---

## 2. Fee Market

### `trv1_getCurrentBaseFee`

Returns the current EIP-1559-style dynamic base fee.

**Availability:** All nodes

**Parameters:** None

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_getCurrentBaseFee",
  "params": []
}
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "baseFeeLamports": 5000,
    "slot": 200000000,
    "utilizationRatio": 0.72,
    "minBaseFeeLamports": 1000,
    "maxBaseFeeLamports": 100000,
    "trend": "up"
  },
  "id": 1
}
```

---

### `trv1_getFeeHistory`

Returns fee statistics for the last N blocks.

**Availability:** All nodes

**Parameters:**

| # | Type | Description |
|---|------|-------------|
| 1 | `u64` | Number of blocks to return (1–1024) |

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_getFeeHistory",
  "params": [3]
}
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "slot": 200000000,
      "baseFeeLamports": 5000,
      "avgPriorityFeeLamports": 1200,
      "medianPriorityFeeLamports": 800,
      "maxPriorityFeeLamports": 50000,
      "transactionCount": 2400,
      "utilizationRatio": 0.72
    },
    {
      "slot": 199999999,
      "baseFeeLamports": 4800,
      "avgPriorityFeeLamports": 1100,
      "medianPriorityFeeLamports": 750,
      "maxPriorityFeeLamports": 45000,
      "transactionCount": 2200,
      "utilizationRatio": 0.68
    },
    {
      "slot": 199999998,
      "baseFeeLamports": 4600,
      "avgPriorityFeeLamports": 950,
      "medianPriorityFeeLamports": 600,
      "maxPriorityFeeLamports": 30000,
      "transactionCount": 2000,
      "utilizationRatio": 0.63
    }
  ],
  "id": 1
}
```

---

### `trv1_estimateFee`

Estimates the total fee for a base64-encoded transaction.

**Availability:** All nodes

**Parameters:**

| # | Type | Description |
|---|------|-------------|
| 1 | `string` | Base64-encoded serialized transaction |

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_estimateFee",
  "params": ["SGVsbG8gV29ybGQ="]
}
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "baseFeeLamports": 5000,
    "recommendedPriorityFeeLamports": 1500,
    "totalEstimatedFeeLamports": 6500,
    "estimatedSlot": 200000005,
    "confidence": "high"
  },
  "id": 1
}
```

---

## 3. Validators

### `trv1_getActiveValidators`

Returns the active validator set (top 200 by delegated stake).

**Availability:** All nodes

**Parameters:** None

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_getActiveValidators",
  "params": []
}
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "identity": "Abc123...",
      "voteAccount": "Def456...",
      "name": "TRv1 Foundation Node",
      "commission": 5,
      "activeStakeLamports": 5000000000000,
      "isActive": true,
      "rank": 1,
      "lastVoteSlot": 200000000,
      "rootSlot": 199999950,
      "isDelinquent": false,
      "epochUptimePct": 99.98,
      "epochBlocksProduced": 1200,
      "epochLeaderSlots": 1204,
      "isJailed": false,
      "version": "4.0.0-alpha.0"
    }
  ],
  "id": 1
}
```

---

### `trv1_getStandbyValidators`

Returns validators in the standby set (ranked 201+).

**Availability:** All nodes

**Parameters:** None

**Request / Response:** Same schema as `trv1_getActiveValidators` with `isActive: false`.

---

### `trv1_getSlashingInfo`

Returns the slashing history for a validator.

**Availability:** All nodes

**Parameters:**

| # | Type | Description |
|---|------|-------------|
| 1 | `string` | Validator identity pubkey (base-58) |

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_getSlashingInfo",
  "params": ["Abc123..."]
}
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "validator": "Abc123...",
    "totalSlashingEvents": 1,
    "totalLamportsSlashed": 50000000000,
    "events": [
      {
        "epoch": 420,
        "slot": 180000000,
        "reason": "double_vote",
        "lamportsSlashed": 50000000000,
        "slashPctBps": 500
      }
    ]
  },
  "id": 1
}
```

---

### `trv1_getJailStatus`

Returns the jail status for a validator.

**Availability:** All nodes

**Parameters:**

| # | Type | Description |
|---|------|-------------|
| 1 | `string` | Validator identity pubkey (base-58) |

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_getJailStatus",
  "params": ["Abc123..."]
}
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "validator": "Abc123...",
    "isJailed": true,
    "jailedSinceEpoch": 448,
    "releaseEpoch": 458,
    "reason": "prolonged_downtime",
    "totalJailCount": 2
  },
  "id": 1
}
```

---

## 4. Treasury

### `trv1_getTreasuryInfo`

Returns the treasury account balance and flow statistics.

**Availability:** All nodes

**Parameters:** None

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_getTreasuryInfo",
  "params": []
}
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "treasuryPubkey": "TRv1Treasury111111111111111111111111111111111",
    "balanceLamports": 250000000000000,
    "totalReceivedLamports": 500000000000000,
    "totalDisbursedLamports": 250000000000000,
    "epoch": 450,
    "epochReceivedLamports": 1200000000000,
    "epochDisbursedLamports": 500000000000,
    "feeShareBps": 1000
  },
  "id": 1
}
```

---

## 5. Governance

### `trv1_getGovernanceConfig`

Returns the governance module configuration.

**Availability:** All nodes

**Parameters:** None

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_getGovernanceConfig",
  "params": []
}
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "programId": "TRv1Gov1111111111111111111111111111111111111",
    "minProposalStakeLamports": 100000000000,
    "votingPeriodSlots": 432000,
    "votingPeriodHours": 48.0,
    "quorumBps": 3000,
    "passThresholdBps": 6667,
    "executionCooldownSlots": 21600,
    "maxActiveProposals": 10
  },
  "id": 1
}
```

---

### `trv1_getProposals`

Returns governance proposals, optionally filtered by status.

**Availability:** RPC nodes only (performs account scan)

**Parameters:**

| # | Type | Description |
|---|------|-------------|
| 1 | `string?` | Optional status filter: `"pending"`, `"active"`, `"passed"`, `"rejected"`, `"executed"`, `"cancelled"` |

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_getProposals",
  "params": ["active"]
}
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "proposalId": 42,
      "proposer": "9aE476sH92Vz7DMPyq5WLPkrKWivxeuTKEFKd2sZZcde",
      "title": "Increase treasury fee share to 12%",
      "description": "This proposal adjusts the treasury fee share from 10% to 12% to fund ecosystem grants.",
      "status": "active",
      "createdEpoch": 448,
      "votingStartSlot": 199500000,
      "votingEndSlot": 199932000,
      "yesStakeLamports": 800000000000000,
      "noStakeLamports": 200000000000000,
      "abstainStakeLamports": 50000000000000,
      "quorumReachedBps": 3500,
      "quorumMet": true,
      "proposalType": "parameter_change",
      "payload": "{\"param\":\"treasury_fee_share_bps\",\"value\":1200}"
    }
  ],
  "id": 1
}
```

---

### `trv1_getProposal`

Returns full details of a single proposal.

**Availability:** All nodes

**Parameters:**

| # | Type | Description |
|---|------|-------------|
| 1 | `u64` | Proposal ID |

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_getProposal",
  "params": [42]
}
```

**Response:** Same schema as individual entries in `trv1_getProposals`.

---

## 6. Developer Rewards

### `trv1_getDevRewardsConfig`

Returns the developer-reward configuration for a program.

**Availability:** All nodes

**Parameters:**

| # | Type | Description |
|---|------|-------------|
| 1 | `string` | Program ID (base-58) |

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_getDevRewardsConfig",
  "params": ["TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"]
}
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    "rewardRecipient": "DevRwrd1111111111111111111111111111111111111",
    "isEnrolled": true,
    "developerShareBps": 500,
    "totalRewardsEarnedLamports": 75000000000,
    "epochRewardsLamports": 1500000000,
    "epochTransactionCount": 350000
  },
  "id": 1
}
```

---

### `trv1_getTopEarningPrograms`

Returns the top-earning programs for the current epoch.

**Availability:** RPC nodes only (performs account scan)

**Parameters:**

| # | Type | Description |
|---|------|-------------|
| 1 | `u64?` | Optional limit (default: 10, max: 100) |

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_getTopEarningPrograms",
  "params": [5]
}
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
      "rewardRecipient": "DevRwrd1111111111111111111111111111111111111",
      "epochEarningsLamports": 1500000000,
      "epochTransactionCount": 350000,
      "rank": 1
    },
    {
      "programId": "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4",
      "rewardRecipient": "JupRcpt1111111111111111111111111111111111111",
      "epochEarningsLamports": 1200000000,
      "epochTransactionCount": 280000,
      "rank": 2
    }
  ],
  "id": 1
}
```

---

## 7. Network Info

### `trv1_getNetworkSummary`

Returns a high-level TRv1 network summary combining staking, fee, validator, and supply data.

**Availability:** All nodes

**Parameters:** None

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_getNetworkSummary",
  "params": []
}
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "epoch": 450,
    "slot": 200000000,
    "blockHeight": 185000000,
    "totalSupplyLamports": 5000000000000000000,
    "circulatingSupplyLamports": 3800000000000000000,
    "stakingParticipationBps": 6800,
    "activeStakeLamports": 2500000000000000000,
    "passiveStakeLamports": 450000000000000,
    "activeValidatorCount": 200,
    "standbyValidatorCount": 150,
    "currentBaseFeeLamports": 5000,
    "inflationRateBps": 50,
    "recentTps": 3500.5,
    "version": "4.0.0-alpha.0"
  },
  "id": 1
}
```

---

### `trv1_getFeeDistribution`

Returns the fee-distribution breakdown for the current epoch.

**Availability:** All nodes

**Parameters:** None

**Request:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "trv1_getFeeDistribution",
  "params": []
}
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "epoch": 450,
    "totalFeesCollectedLamports": 12000000000000,
    "burnedLamports": 6000000000000,
    "treasuryLamports": 1200000000000,
    "validatorLamports": 3000000000000,
    "developerLamports": 600000000000,
    "passiveStakingLamports": 1200000000000,
    "burnShareBps": 5000,
    "treasuryShareBps": 1000,
    "validatorShareBps": 2500,
    "developerShareBps": 500,
    "passiveStakingShareBps": 1000
  },
  "id": 1
}
```

---

## Error Codes

| Code | Meaning |
|------|---------|
| `-32000` | Method not yet implemented (stub) |
| `-32602` | Invalid params (e.g., bad pubkey, `blocks` out of range) |
| `-32600` | Invalid request |

During early development, all TRv1-specific endpoints return error code `-32000` with a descriptive message indicating the method is not yet backed by on-chain state. This allows clients to integrate against the API surface immediately while the on-chain programs are being developed.

---

## Method Summary

| Method | Parameters | Availability |
|--------|-----------|--------------|
| `trv1_getPassiveStakeAccount` | `pubkey: string` | All nodes |
| `trv1_getPassiveStakesByOwner` | `owner: string` | RPC only |
| `trv1_getPassiveStakingRates` | — | All nodes |
| `trv1_getCurrentBaseFee` | — | All nodes |
| `trv1_getFeeHistory` | `blocks: u64` | All nodes |
| `trv1_estimateFee` | `transaction: string` | All nodes |
| `trv1_getActiveValidators` | — | All nodes |
| `trv1_getStandbyValidators` | — | All nodes |
| `trv1_getSlashingInfo` | `validator: string` | All nodes |
| `trv1_getJailStatus` | `validator: string` | All nodes |
| `trv1_getTreasuryInfo` | — | All nodes |
| `trv1_getGovernanceConfig` | — | All nodes |
| `trv1_getProposals` | `status?: string` | RPC only |
| `trv1_getProposal` | `proposal_id: u64` | All nodes |
| `trv1_getDevRewardsConfig` | `program_id: string` | All nodes |
| `trv1_getTopEarningPrograms` | `limit?: u64` | RPC only |
| `trv1_getNetworkSummary` | — | All nodes |
| `trv1_getFeeDistribution` | — | All nodes |
