# Needle test

Smallest pre-LN transformer (heads 4, MLP 2d) with best held-out accuracy >= 95%.

## Length 64

| needles | smallest net >= 95 % | params | best acc |
|---|---|---|---|
| 1 | d64 x L3 | 113440 | 0.999 |
| 3 | d64 x L3 | 113440 | 0.998 |
| 5 | d64 x L3 | 113440 | 1.000 |

## Length 128

| needles | smallest net >= 95 % | params | best acc |
|---|---|---|---|
| 1 | d96 x L3 | 250016 | 0.992 |
| 3 | d64 x L3 | 117536 | 1.000 |
| 5 | d64 x L3 | 117536 | 1.000 |

## Length 256

| needles | smallest net >= 95 % | params | best acc |
|---|---|---|---|
| 1 | none up to d128 L6 (best acc 0.040) | - | 0.040 |
| 3 | d96 x L3 | 262304 | 1.000 |
| 5 | d64 x L3 | 125728 | 0.999 |

## All runs

| length | needles | d | L | params | steps | best | final | seconds | steps/s |
|---|---|---|---|---|---|---|---|---|---|
| 64 | 1 | 64 | 3 | 113440 | 1250 | 0.999 | 0.999 | 3.4 | 372.99 |
| 64 | 1 | 64 | 4 | 146912 | skipped: smaller net passed | | | | |
| 64 | 1 | 64 | 6 | 213856 | skipped: smaller net passed | | | | |
| 64 | 1 | 96 | 3 | 243872 | skipped: smaller net passed | | | | |
| 64 | 1 | 96 | 4 | 318656 | skipped: smaller net passed | | | | |
| 64 | 1 | 128 | 3 | 423456 | skipped: smaller net passed | | | | |
| 64 | 1 | 96 | 6 | 468224 | skipped: smaller net passed | | | | |
| 64 | 1 | 128 | 4 | 555936 | skipped: smaller net passed | | | | |
| 64 | 1 | 128 | 6 | 820896 | skipped: smaller net passed | | | | |
| 64 | 3 | 64 | 3 | 113440 | 500 | 0.998 | 0.998 | 1.3 | 393.68 |
| 64 | 3 | 64 | 4 | 146912 | skipped: smaller net passed | | | | |
| 64 | 3 | 64 | 6 | 213856 | skipped: smaller net passed | | | | |
| 64 | 3 | 96 | 3 | 243872 | skipped: smaller net passed | | | | |
| 64 | 3 | 96 | 4 | 318656 | skipped: smaller net passed | | | | |
| 64 | 3 | 128 | 3 | 423456 | skipped: smaller net passed | | | | |
| 64 | 3 | 96 | 6 | 468224 | skipped: smaller net passed | | | | |
| 64 | 3 | 128 | 4 | 555936 | skipped: smaller net passed | | | | |
| 64 | 3 | 128 | 6 | 820896 | skipped: smaller net passed | | | | |
| 64 | 5 | 64 | 3 | 113440 | 500 | 1.000 | 1.000 | 1.3 | 373.55 |
| 64 | 5 | 64 | 4 | 146912 | skipped: smaller net passed | | | | |
| 64 | 5 | 64 | 6 | 213856 | skipped: smaller net passed | | | | |
| 64 | 5 | 96 | 3 | 243872 | skipped: smaller net passed | | | | |
| 64 | 5 | 96 | 4 | 318656 | skipped: smaller net passed | | | | |
| 64 | 5 | 128 | 3 | 423456 | skipped: smaller net passed | | | | |
| 64 | 5 | 96 | 6 | 468224 | skipped: smaller net passed | | | | |
| 64 | 5 | 128 | 4 | 555936 | skipped: smaller net passed | | | | |
| 64 | 5 | 128 | 6 | 820896 | skipped: smaller net passed | | | | |
| 128 | 1 | 64 | 3 | 117536 | 3000 | 0.037 | 0.037 | 13.6 | 220.86 |
| 128 | 1 | 64 | 4 | 151008 | 3000 | 0.060 | 0.060 | 17.5 | 171.69 |
| 128 | 1 | 64 | 6 | 217952 | 3000 | 0.038 | 0.031 | 25.6 | 117.03 |
| 128 | 1 | 96 | 3 | 250016 | 2250 | 0.992 | 0.992 | 12.8 | 175.53 |
| 128 | 1 | 96 | 4 | 324800 | skipped: smaller net passed | | | | |
| 128 | 1 | 128 | 3 | 431648 | skipped: smaller net passed | | | | |
| 128 | 1 | 96 | 6 | 474368 | skipped: smaller net passed | | | | |
| 128 | 1 | 128 | 4 | 564128 | skipped: smaller net passed | | | | |
| 128 | 1 | 128 | 6 | 829088 | skipped: smaller net passed | | | | |
| 128 | 3 | 64 | 3 | 117536 | 1000 | 1.000 | 1.000 | 4.7 | 212.44 |
| 128 | 3 | 64 | 4 | 151008 | skipped: smaller net passed | | | | |
| 128 | 3 | 64 | 6 | 217952 | skipped: smaller net passed | | | | |
| 128 | 3 | 96 | 3 | 250016 | skipped: smaller net passed | | | | |
| 128 | 3 | 96 | 4 | 324800 | skipped: smaller net passed | | | | |
| 128 | 3 | 128 | 3 | 431648 | skipped: smaller net passed | | | | |
| 128 | 3 | 96 | 6 | 474368 | skipped: smaller net passed | | | | |
| 128 | 3 | 128 | 4 | 564128 | skipped: smaller net passed | | | | |
| 128 | 3 | 128 | 6 | 829088 | skipped: smaller net passed | | | | |
| 128 | 5 | 64 | 3 | 117536 | 750 | 1.000 | 1.000 | 3.6 | 205.91 |
| 128 | 5 | 64 | 4 | 151008 | skipped: smaller net passed | | | | |
| 128 | 5 | 64 | 6 | 217952 | skipped: smaller net passed | | | | |
| 128 | 5 | 96 | 3 | 250016 | skipped: smaller net passed | | | | |
| 128 | 5 | 96 | 4 | 324800 | skipped: smaller net passed | | | | |
| 128 | 5 | 128 | 3 | 431648 | skipped: smaller net passed | | | | |
| 128 | 5 | 96 | 6 | 474368 | skipped: smaller net passed | | | | |
| 128 | 5 | 128 | 4 | 564128 | skipped: smaller net passed | | | | |
| 128 | 5 | 128 | 6 | 829088 | skipped: smaller net passed | | | | |
| 256 | 1 | 64 | 3 | 125728 | 3000 | 0.039 | 0.023 | 39.3 | 76.24 |
| 256 | 1 | 64 | 4 | 159200 | 3000 | 0.039 | 0.030 | 55.4 | 54.2 |
| 256 | 1 | 64 | 6 | 226144 | 3000 | 0.039 | 0.028 | 80.3 | 37.35 |
| 256 | 1 | 96 | 3 | 262304 | 3000 | 0.039 | 0.034 | 48.7 | 61.6 |
| 256 | 1 | 96 | 4 | 337088 | 3000 | 0.039 | 0.023 | 65.5 | 45.78 |
| 256 | 1 | 128 | 3 | 448032 | 3000 | 0.039 | 0.027 | 57.0 | 52.63 |
| 256 | 1 | 96 | 6 | 486656 | 3000 | 0.039 | 0.028 | 100.4 | 29.89 |
| 256 | 1 | 128 | 4 | 580512 | 3000 | 0.038 | 0.026 | 73.3 | 40.9 |
| 256 | 1 | 128 | 6 | 845472 | 3000 | 0.040 | 0.028 | 115.3 | 26.01 |
| 256 | 3 | 64 | 3 | 125728 | 3000 | 0.035 | 0.032 | 40.5 | 74.12 |
| 256 | 3 | 64 | 4 | 159200 | 3000 | 0.060 | 0.060 | 54.0 | 55.54 |
| 256 | 3 | 64 | 6 | 226144 | 3000 | 0.034 | 0.031 | 79.6 | 37.7 |
| 256 | 3 | 96 | 3 | 262304 | 1750 | 1.000 | 1.000 | 28.5 | 61.35 |
| 256 | 3 | 96 | 4 | 337088 | skipped: smaller net passed | | | | |
| 256 | 3 | 128 | 3 | 448032 | skipped: smaller net passed | | | | |
| 256 | 3 | 96 | 6 | 486656 | skipped: smaller net passed | | | | |
| 256 | 3 | 128 | 4 | 580512 | skipped: smaller net passed | | | | |
| 256 | 3 | 128 | 6 | 845472 | skipped: smaller net passed | | | | |
| 256 | 5 | 64 | 3 | 125728 | 1750 | 0.999 | 0.999 | 23.5 | 74.32 |
| 256 | 5 | 64 | 4 | 159200 | skipped: smaller net passed | | | | |
| 256 | 5 | 64 | 6 | 226144 | skipped: smaller net passed | | | | |
| 256 | 5 | 96 | 3 | 262304 | skipped: smaller net passed | | | | |
| 256 | 5 | 96 | 4 | 337088 | skipped: smaller net passed | | | | |
| 256 | 5 | 128 | 3 | 448032 | skipped: smaller net passed | | | | |
| 256 | 5 | 96 | 6 | 486656 | skipped: smaller net passed | | | | |
| 256 | 5 | 128 | 4 | 580512 | skipped: smaller net passed | | | | |
| 256 | 5 | 128 | 6 | 845472 | skipped: smaller net passed | | | | |
