# Mechanics sheet

Values read from the original program (terminal64.exe, a packed x86-64 Windows build) with the
built-in decompiler, and the rules this rebuild implements. "estimated" means the value was looked
for in the code and not found, so a typical value was used instead.

## Built-in indicator defaults

| Indicator | Default parameters | Source |
|---|---|---|
| Bollinger Bands | period 20, deviation 2.0, line colour 0x71b33c (BGR) | function 0x141081de0 ("period=%u" with 0x14; double at 0x141c6e810 = 2.0) |
| MACD | fast EMA 12, slow EMA 26, signal SMA 9; histogram 0xc0c0c0 (silver), signal 0x0000ff (red, BGR) | function 0x1410a7d80 ("fast_ema=%u" 12, "slow_ema=%u" 0x1a, "macd_sma=%u" 9) |
| Relative Strength Index | period 14 | function 0x1410b14b0 ("period=%u" 0xe) |
| Stochastic Oscillator | %K 5, %D 3, slowing 3 | function 0x1410b9150 ("kperiod=%u" 5, "dperiod=%u" 3, "slowing=%u" 3) |
| Ichimoku Kinko Hyo | tenkan 9, kijun 26, senkou 52 | function 0x1410a22e0 |
| Parabolic SAR | step 0.02, maximum 0.2 | function 0x1410b52c0 (doubles at 0x141c83030 = 0.02, 0x141c81610 = 0.2) |
| Average True Range | period 14 | function 0x1410801c0 |
| Commodity Channel Index | period 14 | function 0x141087a00 |
| Momentum | period 14 | function 0x1410ac5b0 |
| Average Directional Movement Index | period 14 | function 0x141076690 |
| Williams' Percent Range | period 14 | function 0x1410b0020 |
| Envelopes | period 14, deviation 0.1 % | function 0x141096050 (double at 0x141c80438 = 0.1) |
| Alligator | periods 13 / 8 / 5, method 2 (smoothed) | function 0x14107a010 ("period1..3=%u", "method=%d" 2) |
| Alligator shifts | 8 / 5 / 3 | estimated |
| Moving Average | period 10 | estimated (the MA setup function was not located) |

Formulas (standard definitions): MACD signal is a simple average of the MACD line (the original
labels it "macd_sma"); RSI uses Wilder smoothing; ATR is a simple average of true range; Bollinger
uses population standard deviation; Stochastic %K uses the slowing sum of (close-LL)/(HH-LL).

## Program runtime (custom indicators, expert advisors, scripts)

Observed in the expert runtime dispatcher, function 0x140e8d7b0, and the event name table at
0x1423a97e8:

- Event handlers: OnInit, OnDeinit, OnTick, OnTimer, OnTrade, OnTradeTransaction, OnChartEvent,
  OnTester, OnBookEvent; indicators use OnCalculate; scripts use OnStart.
- OnInit returning non-zero: "OnInit returned non-zero code %d" and the program is not started.
- Critical runtime errors stop the program: "zero divide", "array out of range",
  "critical runtime error %d in %s function (%s)"; "expert removed itself" after ExpertRemove.
- "endless loop detected in %s function": this rebuild stops a handler after 30,000,000 executed
  statements (limit estimated).

Rules implemented in the engine:

- Programs are compiled from .mq5 source (C-like language with inputs, enums, structs, arrays,
  functions, #property, #define, #include). Classes and templates are not supported yet.
- .ex5 files are recognised but refused with an explanation: that format is encrypted.
- Indicator buffers are resized to the bar count before each OnCalculate; new elements start
  empty (EMPTY_VALUE). prev_calculated is the value OnCalculate returned last time, reset to 0
  when history changes.
- Plots take consecutive data/colour buffers in index order; calculation buffers are skipped.
- CopyBuffer/CopyClose... copy `count` values starting `start` bars back from the newest, oldest
  first; arrays marked with ArraySetAsSeries index 0 as the newest.
- Trading from programs needs the Algo Trading button on (retcode 10027 otherwise); indicators
  cannot trade.

## Simulated account

| Value | Setting | Source |
|---|---|---|
| Default demo deposit | 10,000 USD | estimated |
| Default leverage | 1:100 | estimated |
| Margin call level | 100 % | estimated |
| Stop out level | 50 % (the original has a "Stop Out" message) | estimated level; message observed in 0x140e8d7b0 strings |
| Account mode | hedging (several positions per symbol) | estimated |
| Contract size | 100,000 for currency pairs, 100 for gold, 1 for bitcoin | estimated |
| Trade result codes | 10009 done, 10014 invalid volume, 10016 invalid stops, 10019 no money | standard values of the platform's language |

Profit = price difference x volume x contract size, converted to USD (divided by the price for
USD-based pairs). Margin = volume x contract size x price / leverage (without the price for
USD-based pairs). Stop loss, take profit and pending orders are checked on every tick.

## Chart

| Value | Setting | Source |
|---|---|---|
| Colour scheme | "Green on Black": black background, lime bars, black bull / white bear candles | estimated (colour keys found at 0x14103f280, defaults not located) |
| History per chart | 1,500 bars, generated | estimated |
| Chart shift | 12 % of width | estimated |
| Timeframes | M1 M5 M15 M30 H1 H4 D1 W1 MN1 | standard |

## Strategy tester

Modelling modes: every tick (generated, 13 points per bar), 1 minute OHLC (4 points: open, low/high,
high/low, close), open prices only. Positions still open at the end are closed. Report: net
profit, gross profit/loss, profit factor, expected payoff, maximal drawdown, trade counts.
