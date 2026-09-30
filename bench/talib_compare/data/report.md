# fin-primitives vs TA-Lib: 100000 bars, CPU 13th Gen Intel(R) Core(TM) i7-13700KF
TA-Lib 0.8.1 (Sep 21 2026 16:39:26) (Python TA-Lib 0.8.1, numpy 2.5.3, Python 3.12.10)

## Agreement (max absolute difference)

| indicator | first value (fp / TA-Lib) | all common bars | after bar 1000 | bars > 1e-9 after 1000 | vs matching formula |
|---|---|---|---|---|---|
| SMA_20 | 19 / 19 | 9.8e-13 | 9.8e-13 | 0 |  |
| EMA_20 | 19 / 19 | 5.7e-14 | 5.7e-14 | 0 |  |
| WMA_20 | 19 / 19 | 2.8e-12 | 2.8e-12 | 0 |  |
| DEMA_20 | 38 / 38 | 1.4e-13 | 1.4e-13 | 0 |  |
| TEMA_20 | 57 / 57 | 3.1e-13 | 3.1e-13 | 0 |  |
| TRIX_15 | 43 / 43 | 6.6e-02 | 2.6e-14 | 0 |  |
| KAMA_10 | 10 / 10 | 2.0e-02 | 1.1e-13 | 0 |  |
| RSI_14 | 14 / 14 | 1.8e-13 | 1.8e-13 | 0 |  |
| MACDHIST_12_26_9 | 33 / 33 | 3.6e-02 | 5.0e-14 | 0 |  |
| BBANDS_UPPER_20_2 | 19 / 19 | 1.6e-11 | 1.6e-11 | 0 |  |
| BBANDS_MIDDLE_20_2 | 19 / 19 | 9.8e-13 | 9.8e-13 | 0 |  |
| BBANDS_LOWER_20_2 | 19 / 19 | 1.6e-11 | 1.6e-11 | 0 |  |
| BBANDS_PCTB_20_2 | 19 / 19 | 3.4e-12 | 3.4e-12 | 0 |  |
| ATR_14 | 14 / 14 | 7.1e-01 | 7.1e-01 | 99000 | SMA(TRANGE, 14): 1.2e-14 |
| NATR_14 | 14 / 14 | 4.9e-01 | 4.9e-01 | 99000 | SMA(TRANGE, 14) / close * 100: 8.4e-15 |
| STOCHF_K_14 | 13 / 15 | 6.7e-13 | 6.7e-13 | 0 |  |
| STOCHF_D_14_3 | 15 / 15 | 1.3e-12 | 1.3e-12 | 0 |  |
| WILLR_14 | 13 / 13 | 6.7e-13 | 6.7e-13 | 0 |  |
| CCI_20 | 19 / 19 | 1.0e-11 | 1.0e-11 | 0 |  |
| ROC_10 | 10 / 10 | 3.1e-14 | 3.1e-14 | 0 |  |
| MOM_10 | 10 / 10 | 2.8e-14 | 2.8e-14 | 0 |  |
| MFI_14 | 14 / 14 | 1.3e-12 | 1.3e-12 | 0 |  |
| ADX_14 | 28 / 27 | 2.3e+01 | 2.4e+00 | 11375 |  |
| AROONOSC_14 | 14 / 14 | 1.4e-14 | 1.4e-14 | 0 |  |
| AROON_UP_14 | 14 / 14 | 1.4e-14 | 1.4e-14 | 0 |  |
| AROON_DOWN_14 | 14 / 14 | 1.4e-14 | 1.4e-14 | 0 |  |
| OBV | 0 / 0 | 1.1e+06 | 1.1e+06 | 99000 | OBV - volume[0]: 0 |

ADX: 128 bars have up_move == down_move > 0 exactly; in float64 39 of them compare unequal. Max ADX difference on bars after 1000 that are more than 600 bars past such a tie: 3.7e-13.

Bollinger upper band vs a 40-digit decimal reference (every 37th bar): TA-Lib max error 1.2e-11, fin-primitives max error 0.

## Throughput (ns per bar, lower is better)

| indicator | fin-primitives streaming update() | TA-Lib batch (whole array) | TA-Lib per bar from Python (talib.stream, 200-bar window) |
|---|---|---|---|
| SMA_20 | 74 | 0.8 | 650 |
| EMA_20 | 149 | 1.3 | 716 |
| WMA_20 | 106 | 0.8 |  |
| DEMA_20 | 355 | 1.5 |  |
| TEMA_20 | 569 | 3.3 |  |
| TRIX_15 | 305 | 1.7 |  |
| KAMA_10 | 401 | 2.9 |  |
| RSI_14 | 252 | 2.1 | 877 |
| MACDHIST_12_26_9 | 362 | 1.7 | 1044 |
| BBANDS_PCTB_20_2 | 1023 | 2.7 | 1743 |
| ATR_14 | 86 | 0.8 | 749 |
| NATR_14 | 141 | 1.1 |  |
| STOCHF_K_14 | 174 | 7.7 | 1708 |
| STOCHF_D_14_3 | 259 | 7.7 |  |
| WILLR_14 | 170 | 1.9 |  |
| CCI_20 | 1060 | 12.2 |  |
| ROC_10 | 38 | 0.8 |  |
| MOM_10 | 7 | 0.2 |  |
| MFI_14 | 589 | 5.9 |  |
| ADX_14 | 794 | 6.2 |  |
| AROONOSC_14 | 205 | 6.6 |  |
| OBV | 13 | 2.8 | 710 |
