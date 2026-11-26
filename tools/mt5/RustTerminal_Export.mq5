//+------------------------------------------------------------------+
//| RustTerminal_Export.mq5                                          |
//| Run this script in MetaTrader 5 to export price history and      |
//| symbol properties for Rust Terminal.                             |
//| Files go to <Common Files>/RustTerminal:                         |
//|   <SYMBOL>_<TF>.csv  bars (MetaTrader export layout)             |
//|   symbols.csv        digits, contract size, volumes, currencies  |
//| Rust Terminal: File > Import from MetaTrader 5.                  |
//+------------------------------------------------------------------+
#property copyright "Rust Terminal"
#property version   "1.00"
#property script_show_inputs

input string          InpSymbols   = "";          // Symbols, comma separated (empty = Market Watch)
input ENUM_TIMEFRAMES InpTimeframe = PERIOD_M1;   // Timeframe
input int             InpDays      = 180;         // Days of history

string TfLabel(ENUM_TIMEFRAMES tf)
  {
   string s = EnumToString(tf);          // "PERIOD_M1"
   return StringSubstr(s, 7);
  }

string Clean(string s)
  {
   StringReplace(s, ";", ",");
   StringReplace(s, "\r", " ");
   StringReplace(s, "\n", " ");
   return s;
  }

bool ExportBars(string sym, ENUM_TIMEFRAMES tf, int days, int &count)
  {
   count = 0;
   datetime to   = TimeCurrent();
   datetime from = to - (datetime)days * 86400;
   MqlRates rates[];
   int got = CopyRates(sym, tf, from, to, rates);
   if(got <= 0)
     {
      PrintFormat("%s: no bars (%d), error %d", sym, got, GetLastError());
      return false;
     }
   int digits = (int)SymbolInfoInteger(sym, SYMBOL_DIGITS);
   string name = "RustTerminal\\" + sym + "_" + TfLabel(tf) + ".csv";
   int h = FileOpen(name, FILE_WRITE | FILE_TXT | FILE_ANSI | FILE_COMMON);
   if(h == INVALID_HANDLE)
     {
      PrintFormat("%s: cannot write %s, error %d", sym, name, GetLastError());
      return false;
     }
   FileWriteString(h, "<DATE>\t<TIME>\t<OPEN>\t<HIGH>\t<LOW>\t<CLOSE>\t<TICKVOL>\t<VOL>\t<SPREAD>\r\n");
   for(int i = 0; i < got; i++)
     {
      string line = TimeToString(rates[i].time, TIME_DATE) + "\t" +
                    TimeToString(rates[i].time, TIME_SECONDS) + "\t" +
                    DoubleToString(rates[i].open, digits) + "\t" +
                    DoubleToString(rates[i].high, digits) + "\t" +
                    DoubleToString(rates[i].low, digits) + "\t" +
                    DoubleToString(rates[i].close, digits) + "\t" +
                    IntegerToString(rates[i].tick_volume) + "\t" +
                    IntegerToString(rates[i].real_volume) + "\t" +
                    IntegerToString(rates[i].spread) + "\r\n";
      FileWriteString(h, line);
     }
   FileClose(h);
   count = got;
   return true;
  }

void OnStart()
  {
   string list[];
   int n = 0;
   if(StringLen(InpSymbols) > 0)
      n = StringSplit(InpSymbols, ',', list);
   else
     {
      n = SymbolsTotal(true);
      ArrayResize(list, n);
      for(int i = 0; i < n; i++)
         list[i] = SymbolName(i, true);
     }
   FolderCreate("RustTerminal", FILE_COMMON);
   // the broker's offset from UTC, so the importer can turn bar times into UTC
   int hz = FileOpen("RustTerminal\\server_zone.txt", FILE_WRITE | FILE_TXT | FILE_ANSI | FILE_COMMON);
   if(hz != INVALID_HANDLE)
     {
      FileWriteString(hz, IntegerToString((long)(TimeTradeServer() - TimeGMT())) + "\r\n");
      FileClose(hz);
     }
   int hs = FileOpen("RustTerminal\\symbols.csv", FILE_WRITE | FILE_TXT | FILE_ANSI | FILE_COMMON);
   if(hs == INVALID_HANDLE)
     {
      Print("cannot write symbols.csv, error ", GetLastError());
      return;
     }
   FileWriteString(hs, "name;description;digits;contract;vmin;vmax;vstep;spread;base;profit;margin;tick_value;tick_size;bid\r\n");
   int done = 0;
   for(int i = 0; i < n && !IsStopped(); i++)
     {
      string sym = list[i];
      StringTrimLeft(sym);
      StringTrimRight(sym);
      if(StringLen(sym) == 0 || !SymbolSelect(sym, true))
         continue;
      int digits = (int)SymbolInfoInteger(sym, SYMBOL_DIGITS);
      string row = StringFormat("%s;%s;%d;%.2f;%.4f;%.4f;%.4f;%d;%s;%s;%s;%.10f;%.10f;%s\r\n",
                                sym,
                                Clean(SymbolInfoString(sym, SYMBOL_DESCRIPTION)),
                                digits,
                                SymbolInfoDouble(sym, SYMBOL_TRADE_CONTRACT_SIZE),
                                SymbolInfoDouble(sym, SYMBOL_VOLUME_MIN),
                                SymbolInfoDouble(sym, SYMBOL_VOLUME_MAX),
                                SymbolInfoDouble(sym, SYMBOL_VOLUME_STEP),
                                (int)SymbolInfoInteger(sym, SYMBOL_SPREAD),
                                SymbolInfoString(sym, SYMBOL_CURRENCY_BASE),
                                SymbolInfoString(sym, SYMBOL_CURRENCY_PROFIT),
                                SymbolInfoString(sym, SYMBOL_CURRENCY_MARGIN),
                                SymbolInfoDouble(sym, SYMBOL_TRADE_TICK_VALUE),
                                SymbolInfoDouble(sym, SYMBOL_TRADE_TICK_SIZE),
                                DoubleToString(SymbolInfoDouble(sym, SYMBOL_BID), digits));
      FileWriteString(hs, row);
      int count = 0;
      if(ExportBars(sym, InpTimeframe, InpDays, count))
        {
         done++;
         PrintFormat("%s: %d bars exported", sym, count);
        }
     }
   FileClose(hs);
   string msg = StringFormat("Rust Terminal export: %d symbols written to Common Files\\RustTerminal", done);
   Print(msg);
   Comment(msg);
  }
