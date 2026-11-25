//! Example programs written for this terminal, copied into the MQL5 folder on first start.

pub const SAMPLES: &[(&str, &str)] = &[
    ("Indicators/Smoothed Price Line.mq5", SMOOTHED_LINE),
    ("Indicators/RSI Color Histogram.mq5", RSI_HIST),
    ("Indicators/Price Channel.mq5", PRICE_CHANNEL),
    ("Indicators/Heikin Ashi Candles.mq5", HEIKIN),
    ("Experts/MA Crossover EA.mq5", MA_CROSS_EA),
    ("Experts/RSI Reversal EA.mq5", RSI_EA),
    ("Scripts/Close All Positions.mq5", CLOSE_ALL),
    ("Scripts/Draw Day Levels.mq5", DRAW_LEVELS),
];

pub const TEMPLATE_INDICATOR: &str = r#"//+------------------------------------------------------------------+
//| New custom indicator                                             |
//+------------------------------------------------------------------+
#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrDodgerBlue
#property indicator_width1  2
#property indicator_label1  "Value"

input int InpPeriod = 14; // Period

double Buffer[];

int OnInit()
  {
   SetIndexBuffer(0, Buffer, INDICATOR_DATA);
   IndicatorSetString(INDICATOR_SHORTNAME, "MyIndicator(" + IntegerToString(InpPeriod) + ")");
   return(INIT_SUCCEEDED);
  }

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
  {
   int start = MathMax(prev_calculated - 1, InpPeriod);
   for(int i = start; i < rates_total; i++)
     {
      double sum = 0;
      for(int k = 0; k < InpPeriod; k++)
         sum += close[i - k];
      Buffer[i] = sum / InpPeriod;
     }
   return(rates_total);
  }
"#;

pub const TEMPLATE_EXPERT: &str = r#"//+------------------------------------------------------------------+
//| New expert advisor                                               |
//+------------------------------------------------------------------+
#include <Trade\Trade.mqh>

input double InpLots  = 0.10; // Lots
input int    InpMagic = 1001; // Magic number

CTrade trade;

int OnInit()
  {
   trade.SetExpertMagicNumber(InpMagic);
   Print("Expert started on ", _Symbol);
   return(INIT_SUCCEEDED);
  }

void OnDeinit(const int reason)
  {
   Print("Expert stopped, reason ", reason);
  }

void OnTick()
  {
   // your trading logic here
  }
"#;

pub const TEMPLATE_SCRIPT: &str = r#"//+------------------------------------------------------------------+
//| New script                                                       |
//+------------------------------------------------------------------+
#property script_show_inputs

input string InpMessage = "Hello"; // Message

void OnStart()
  {
   Print(InpMessage, " from ", _Symbol, " bid=", DoubleToString(SymbolInfoDouble(_Symbol, SYMBOL_BID), _Digits));
  }
"#;

const SMOOTHED_LINE: &str = r#"//+------------------------------------------------------------------+
//| Smoothed Price Line: a moving average you can tune               |
//+------------------------------------------------------------------+
#property description "Moving average drawn by a custom indicator"
#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrGold
#property indicator_width1  2
#property indicator_label1  "Smoothed"

input int            InpPeriod = 21;        // Period
input ENUM_MA_METHOD InpMethod = MODE_EMA;  // Method

double Line[];

int OnInit()
  {
   SetIndexBuffer(0, Line, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod);
   IndicatorSetString(INDICATOR_SHORTNAME, "Smoothed(" + IntegerToString(InpPeriod) + ")");
   return(INIT_SUCCEEDED);
  }

int OnCalculate(const int rates_total, const int prev_calculated, const int begin, const double &price[])
  {
   if(rates_total < InpPeriod) return(0);
   int start = prev_calculated - 1;
   if(start < InpPeriod)
     {
      double sum = 0.0;
      for(int i = 0; i < InpPeriod; i++)
        {
         sum += price[i];
         Line[i] = EMPTY_VALUE;
        }
      Line[InpPeriod - 1] = sum / InpPeriod;
      start = InpPeriod;
     }
   double k = 2.0 / (InpPeriod + 1.0);
   for(int i = start; i < rates_total; i++)
     {
      switch(InpMethod)
        {
         case MODE_EMA:
            Line[i] = price[i] * k + Line[i - 1] * (1.0 - k);
            break;
         case MODE_SMMA:
            Line[i] = (Line[i - 1] * (InpPeriod - 1) + price[i]) / InpPeriod;
            break;
         default:
           {
            double s = 0.0;
            for(int j = 0; j < InpPeriod; j++) s += price[i - j];
            Line[i] = s / InpPeriod;
           }
        }
     }
   return(rates_total);
  }
"#;

const RSI_HIST: &str = r#"//+------------------------------------------------------------------+
//| RSI Color Histogram: RSI minus 50, green above, red below        |
//+------------------------------------------------------------------+
#property indicator_separate_window
#property indicator_buffers 3
#property indicator_plots   1
#property indicator_type1   DRAW_COLOR_HISTOGRAM
#property indicator_color1  clrLimeGreen, clrTomato
#property indicator_width1  3
#property indicator_label1  "RSI-50"
#property indicator_level1  -20
#property indicator_level2  20

input int InpRsiPeriod = 14; // RSI period

double Hist[];
double Colors[];
double Rsi[];
int    rsiHandle = INVALID_HANDLE;

int OnInit()
  {
   SetIndexBuffer(0, Hist, INDICATOR_DATA);
   SetIndexBuffer(1, Colors, INDICATOR_COLOR_INDEX);
   SetIndexBuffer(2, Rsi, INDICATOR_CALCULATIONS);
   rsiHandle = iRSI(_Symbol, _Period, InpRsiPeriod, PRICE_CLOSE);
   if(rsiHandle == INVALID_HANDLE)
     {
      Print("failed to create RSI handle");
      return(INIT_FAILED);
     }
   IndicatorSetString(INDICATOR_SHORTNAME, "RSI Hist(" + IntegerToString(InpRsiPeriod) + ")");
   IndicatorSetInteger(INDICATOR_DIGITS, 2);
   return(INIT_SUCCEEDED);
  }

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
  {
   if(CopyBuffer(rsiHandle, 0, 0, rates_total, Rsi) <= 0) return(0);
   int start = (prev_calculated > 0) ? prev_calculated - 1 : 0;
   for(int i = start; i < rates_total; i++)
     {
      if(Rsi[i] == EMPTY_VALUE) { Hist[i] = EMPTY_VALUE; Colors[i] = 0; continue; }
      Hist[i] = Rsi[i] - 50.0;
      Colors[i] = (Hist[i] >= 0) ? 0 : 1;
     }
   return(rates_total);
  }
"#;

const PRICE_CHANNEL: &str = r#"//+------------------------------------------------------------------+
//| Price Channel: highest high / lowest low with a filled band      |
//+------------------------------------------------------------------+
#property indicator_chart_window
#property indicator_buffers 3
#property indicator_plots   2
#property indicator_type1   DRAW_FILLING
#property indicator_color1  C'40,60,90', C'90,40,40'
#property indicator_label1  "Channel"
#property indicator_type2   DRAW_LINE
#property indicator_color2  clrSilver
#property indicator_style2  STYLE_DOT
#property indicator_label2  "Middle"

input int InpBars = 20; // Bars

double Upper[];
double Lower[];
double Middle[];

int OnInit()
  {
   SetIndexBuffer(0, Upper, INDICATOR_DATA);
   SetIndexBuffer(1, Lower, INDICATOR_DATA);
   SetIndexBuffer(2, Middle, INDICATOR_DATA);
   IndicatorSetString(INDICATOR_SHORTNAME, "Channel(" + IntegerToString(InpBars) + ")");
   return(INIT_SUCCEEDED);
  }

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
  {
   int start = MathMax(prev_calculated - 1, InpBars - 1);
   for(int i = start; i < rates_total; i++)
     {
      double hh = high[i], ll = low[i];
      for(int k = 1; k < InpBars; k++)
        {
         hh = MathMax(hh, high[i - k]);
         ll = MathMin(ll, low[i - k]);
        }
      Upper[i] = hh;
      Lower[i] = ll;
      Middle[i] = (hh + ll) / 2.0;
     }
   return(rates_total);
  }
"#;

const HEIKIN: &str = r#"//+------------------------------------------------------------------+
//| Heikin Ashi Candles drawn in a separate window                   |
//+------------------------------------------------------------------+
#property indicator_separate_window
#property indicator_buffers 5
#property indicator_plots   1
#property indicator_type1   DRAW_COLOR_CANDLES
#property indicator_color1  clrDodgerBlue, clrOrangeRed
#property indicator_label1  "HA Open;HA High;HA Low;HA Close"

double HaOpen[], HaHigh[], HaLow[], HaClose[], HaColor[];

int OnInit()
  {
   SetIndexBuffer(0, HaOpen, INDICATOR_DATA);
   SetIndexBuffer(1, HaHigh, INDICATOR_DATA);
   SetIndexBuffer(2, HaLow, INDICATOR_DATA);
   SetIndexBuffer(3, HaClose, INDICATOR_DATA);
   SetIndexBuffer(4, HaColor, INDICATOR_COLOR_INDEX);
   IndicatorSetString(INDICATOR_SHORTNAME, "Heikin Ashi");
   return(INIT_SUCCEEDED);
  }

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
  {
   int start = prev_calculated - 1;
   if(start < 1)
     {
      HaOpen[0] = open[0]; HaClose[0] = close[0]; HaHigh[0] = high[0]; HaLow[0] = low[0]; HaColor[0] = 0;
      start = 1;
     }
   for(int i = start; i < rates_total; i++)
     {
      double c = (open[i] + high[i] + low[i] + close[i]) / 4.0;
      double o = (HaOpen[i - 1] + HaClose[i - 1]) / 2.0;
      HaOpen[i] = o;
      HaClose[i] = c;
      HaHigh[i] = MathMax(high[i], MathMax(o, c));
      HaLow[i] = MathMin(low[i], MathMin(o, c));
      HaColor[i] = (c >= o) ? 0 : 1;
     }
   return(rates_total);
  }
"#;

const MA_CROSS_EA: &str = r#"//+------------------------------------------------------------------+
//| MA Crossover EA: buys when the fast average crosses above the    |
//| slow one, sells on the opposite cross. One position at a time.   |
//+------------------------------------------------------------------+
#include <Trade\Trade.mqh>

input int    InpFast     = 9;     // Fast MA period
input int    InpSlow     = 21;    // Slow MA period
input double InpLots     = 0.10;  // Lots
input int    InpStopLoss = 300;   // Stop loss (points)
input int    InpTakeProfit = 600; // Take profit (points)
input int    InpMagic    = 20240; // Magic number

CTrade   trade;
int      fastHandle, slowHandle;
datetime lastBar = 0;

int OnInit()
  {
   if(InpFast >= InpSlow)
     {
      Print("Fast period must be smaller than slow period");
      return(INIT_PARAMETERS_INCORRECT);
     }
   fastHandle = iMA(_Symbol, _Period, InpFast, 0, MODE_EMA, PRICE_CLOSE);
   slowHandle = iMA(_Symbol, _Period, InpSlow, 0, MODE_EMA, PRICE_CLOSE);
   trade.SetExpertMagicNumber(InpMagic);
   PrintFormat("MA Crossover started: fast=%d slow=%d lots=%.2f", InpFast, InpSlow, InpLots);
   return(INIT_SUCCEEDED);
  }

void OnDeinit(const int reason)
  {
   IndicatorRelease(fastHandle);
   IndicatorRelease(slowHandle);
   Comment("");
  }

bool HasPosition(long &type)
  {
   for(int i = PositionsTotal() - 1; i >= 0; i--)
     {
      ulong ticket = PositionGetTicket(i);
      if(ticket == 0) continue;
      if(PositionGetString(POSITION_SYMBOL) == _Symbol && PositionGetInteger(POSITION_MAGIC) == InpMagic)
        {
         type = PositionGetInteger(POSITION_TYPE);
         return(true);
        }
     }
   return(false);
  }

void OnTick()
  {
   datetime bar = iTime(_Symbol, _Period, 0);
   if(bar == lastBar) return;   // act once per new bar
   lastBar = bar;

   double fast[], slow[];
   ArraySetAsSeries(fast, true);
   ArraySetAsSeries(slow, true);
   if(CopyBuffer(fastHandle, 0, 1, 2, fast) < 2) return;
   if(CopyBuffer(slowHandle, 0, 1, 2, slow) < 2) return;

   bool crossUp   = fast[1] <= slow[1] && fast[0] > slow[0];
   bool crossDown = fast[1] >= slow[1] && fast[0] < slow[0];

   Comment("MA Crossover\nFast: ", DoubleToString(fast[0], _Digits), "\nSlow: ", DoubleToString(slow[0], _Digits));

   long type = -1;
   bool has = HasPosition(type);
   double ask = SymbolInfoDouble(_Symbol, SYMBOL_ASK);
   double bid = SymbolInfoDouble(_Symbol, SYMBOL_BID);

   if(crossUp)
     {
      if(has && type == POSITION_TYPE_SELL) trade.PositionClose(_Symbol);
      if(!has || type == POSITION_TYPE_SELL)
         trade.Buy(InpLots, _Symbol, ask, ask - InpStopLoss * _Point, ask + InpTakeProfit * _Point, "MA cross buy");
     }
   else if(crossDown)
     {
      if(has && type == POSITION_TYPE_BUY) trade.PositionClose(_Symbol);
      if(!has || type == POSITION_TYPE_BUY)
         trade.Sell(InpLots, _Symbol, bid, bid + InpStopLoss * _Point, bid - InpTakeProfit * _Point, "MA cross sell");
     }
  }
"#;

const RSI_EA: &str = r#"//+------------------------------------------------------------------+
//| RSI Reversal EA: sells when RSI leaves overbought, buys when it  |
//| leaves oversold. Uses OrderSend with a trade request structure.  |
//+------------------------------------------------------------------+
input int    InpPeriod = 14;   // RSI period
input double InpUpper  = 70;   // Overbought level
input double InpLower  = 30;   // Oversold level
input double InpLots   = 0.10; // Lots
input int    InpSLPoints = 400; // Stop loss (points)
input int    InpTPPoints = 400; // Take profit (points)

int rsi;

int OnInit()
  {
   rsi = iRSI(_Symbol, _Period, InpPeriod, PRICE_CLOSE);
   EventSetTimer(30);
   return(rsi == INVALID_HANDLE ? INIT_FAILED : INIT_SUCCEEDED);
  }

void OnDeinit(const int reason) { EventKillTimer(); }

void OnTimer()
  {
   Print("RSI EA alive, equity ", DoubleToString(AccountInfoDouble(ACCOUNT_EQUITY), 2));
  }

bool Send(ENUM_ORDER_TYPE type)
  {
   MqlTradeRequest req;
   MqlTradeResult  res;
   ZeroMemory(req);
   ZeroMemory(res);
   double price = (type == ORDER_TYPE_BUY) ? SymbolInfoDouble(_Symbol, SYMBOL_ASK) : SymbolInfoDouble(_Symbol, SYMBOL_BID);
   double dir = (type == ORDER_TYPE_BUY) ? 1.0 : -1.0;
   req.action = TRADE_ACTION_DEAL;
   req.symbol = _Symbol;
   req.volume = InpLots;
   req.type   = type;
   req.price  = price;
   req.sl     = NormalizeDouble(price - dir * InpSLPoints * _Point, _Digits);
   req.tp     = NormalizeDouble(price + dir * InpTPPoints * _Point, _Digits);
   req.magic  = 777;
   req.comment = "RSI reversal";
   if(!OrderSend(req, res))
     {
      PrintFormat("OrderSend failed, retcode=%d (%s)", res.retcode, res.comment);
      return(false);
     }
   PrintFormat("Opened #%d at %.5f", res.order, res.price);
   return(true);
  }

void OnTick()
  {
   double v[];
   ArraySetAsSeries(v, true);
   if(CopyBuffer(rsi, 0, 0, 3, v) < 3) return;
   if(PositionsTotal() > 0) return;
   if(v[2] > InpUpper && v[1] <= InpUpper) Send(ORDER_TYPE_SELL);
   if(v[2] < InpLower && v[1] >= InpLower) Send(ORDER_TYPE_BUY);
  }
"#;

const CLOSE_ALL: &str = r#"//+------------------------------------------------------------------+
//| Close All Positions on the current symbol (or all symbols)       |
//+------------------------------------------------------------------+
#include <Trade\Trade.mqh>
#property script_show_inputs

input bool InpAllSymbols = false; // Close positions on all symbols

void OnStart()
  {
   CTrade trade;
   int closed = 0;
   for(int i = PositionsTotal() - 1; i >= 0; i--)
     {
      ulong ticket = PositionGetTicket(i);
      if(ticket == 0) continue;
      if(!InpAllSymbols && PositionGetString(POSITION_SYMBOL) != _Symbol) continue;
      if(trade.PositionClose(ticket)) closed++;
     }
   PrintFormat("Closed %d position(s)", closed);
  }
"#;

const DRAW_LEVELS: &str = r#"//+------------------------------------------------------------------+
//| Draw Day Levels: yesterday's high, low and close as lines        |
//+------------------------------------------------------------------+
void OnStart()
  {
   double hi = iHigh(_Symbol, PERIOD_D1, 1);
   double lo = iLow(_Symbol, PERIOD_D1, 1);
   double cl = iClose(_Symbol, PERIOD_D1, 1);
   string names[3] = {"DayHigh", "DayLow", "DayClose"};
   double prices[3];
   prices[0] = hi; prices[1] = lo; prices[2] = cl;
   color colors[3] = {clrLimeGreen, clrTomato, clrGold};
   for(int i = 0; i < 3; i++)
     {
      ObjectDelete(0, names[i]);
      ObjectCreate(0, names[i], OBJ_HLINE, 0, 0, prices[i]);
      ObjectSetInteger(0, names[i], OBJPROP_COLOR, colors[i]);
      ObjectSetInteger(0, names[i], OBJPROP_STYLE, STYLE_DASH);
      ObjectSetString(0, names[i], OBJPROP_TEXT, names[i]);
     }
   ObjectCreate(0, "DayInfo", OBJ_LABEL, 0, 0, 0);
   ObjectSetInteger(0, "DayInfo", OBJPROP_CORNER, CORNER_RIGHT_LOWER);
   ObjectSetInteger(0, "DayInfo", OBJPROP_XDISTANCE, 200);
   ObjectSetInteger(0, "DayInfo", OBJPROP_YDISTANCE, 30);
   ObjectSetString(0, "DayInfo", OBJPROP_TEXT, StringFormat("Yesterday H %.5f  L %.5f", hi, lo));
   Print("Day levels drawn for ", _Symbol);
  }
"#;
