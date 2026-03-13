# Deferred Backlog

Items deferred from the SITREP Comparison Research implementation (March 2026).

## 5.10 Latency-Aware Fusion
Model per-source reporting delay (GDELT ~15min, FIRMS ~3hr). Handle out-of-sequence events by adjusting correlation windows based on known source latency profiles.

**Status:** Not planned.

## L4 Analyst Feedback Loop
Add confirm/reject buttons on situations. Store analyst feedback and use it to adjust scoring weights over time, creating a supervised learning loop for situation quality.

**Status:** Not planned.

## 5.9 Causal Rule Mining
Auto-discover correlation rules from historical data using frequent pattern mining. Requires significant data accumulation before being viable.

**Status:** Not planned.

## 5.4 Multi-Window Burst Detection
Add additional EWMA windows at 1min, 30min, 2hr, and 12hr intervals alongside existing 5min/6hr dual window. Marginal improvement expected over current dual-window approach.

**Status:** Deferred.
