"""Step 1.10 (rl3): RL of the army net on every zone, Rust actors and an MLX learner.

Rust plays (sampling mode `ARMY_SAMPLE_PLAN`, `ai-exam suite --record`), MLX only learns: `learner` reads the records, `scenes`
chooses the scenes of an update, `loop` is the whole loop (actor, learner, anchor, Adam, export, checkpoints, validation in Rust).
The MLX game twin is not used here (frozen, 25 Nov).
"""
