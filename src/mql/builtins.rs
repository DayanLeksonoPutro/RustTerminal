//! Built-in names of the script language: constants, structures, classes and the list of
//! functions the interpreter implements.

use super::ast::Ty;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug)]
pub enum Const {
    I(i64),
    D(f64),
}

impl Const {
    pub fn as_i64(self) -> Option<i64> {
        match self {
            Const::I(v) => Some(v),
            Const::D(v) => Some(v as i64),
        }
    }
    pub fn as_f64(self) -> Option<f64> {
        match self {
            Const::I(v) => Some(v as f64),
            Const::D(v) => Some(v),
        }
    }
}

pub const BUILTIN_CLASSES: [&str; 5] = ["CTrade", "CPositionInfo", "CSymbolInfo", "CAccountInfo", "COrderInfo"];

pub fn builtin_structs() -> Vec<(&'static str, Vec<(&'static str, Ty)>)> {
    use Ty::*;
    vec![
        (
            "MqlTick",
            vec![("time", Int), ("bid", Double), ("ask", Double), ("last", Double), ("volume", Int), ("time_msc", Int), ("flags", Int), ("volume_real", Double)],
        ),
        (
            "MqlRates",
            vec![("time", Int), ("open", Double), ("high", Double), ("low", Double), ("close", Double), ("tick_volume", Int), ("spread", Int), ("real_volume", Int)],
        ),
        (
            "MqlTradeRequest",
            vec![
                ("action", Int),
                ("magic", Int),
                ("order", Int),
                ("symbol", Str),
                ("volume", Double),
                ("price", Double),
                ("stoplimit", Double),
                ("sl", Double),
                ("tp", Double),
                ("deviation", Int),
                ("type", Int),
                ("type_filling", Int),
                ("type_time", Int),
                ("expiration", Int),
                ("comment", Str),
                ("position", Int),
                ("position_by", Int),
            ],
        ),
        (
            "MqlTradeResult",
            vec![
                ("retcode", Int),
                ("deal", Int),
                ("order", Int),
                ("volume", Double),
                ("price", Double),
                ("bid", Double),
                ("ask", Double),
                ("comment", Str),
                ("request_id", Int),
                ("retcode_external", Int),
            ],
        ),
        (
            "MqlTradeCheckResult",
            vec![
                ("retcode", Int),
                ("balance", Double),
                ("equity", Double),
                ("profit", Double),
                ("margin", Double),
                ("margin_free", Double),
                ("margin_level", Double),
                ("comment", Str),
            ],
        ),
        (
            "MqlDateTime",
            vec![("year", Int), ("mon", Int), ("day", Int), ("hour", Int), ("min", Int), ("sec", Int), ("day_of_week", Int), ("day_of_year", Int)],
        ),
    ]
}

/// Variables that are always available.
pub const PREDEFINED_VARS: [&str; 12] =
    ["_Symbol", "_Period", "_Point", "_Digits", "_LastError", "_StopFlag", "_UninitReason", "Bid", "Ask", "_RandomSeed", "_IsX64", "__comma"];

pub const BUILTIN_FUNCS: &[&str] = &[
    "__comma", "Print", "PrintFormat", "Comment", "Alert", "StringFormat", "DoubleToString", "IntegerToString", "StringToDouble",
    "StringToInteger", "TimeToString", "StringToTime", "CharToString", "ShortToString", "ColorToString", "NormalizeDouble",
    "EnumToString", "MathAbs", "fabs", "MathMax", "fmax", "MathMin", "fmin", "MathPow", "pow", "MathSqrt", "sqrt", "MathFloor",
    "floor", "MathCeil", "ceil", "MathRound", "round", "MathLog", "log", "MathLog10", "log10", "MathExp", "exp", "MathSin", "sin",
    "MathCos", "cos", "MathTan", "tan", "MathArctan", "atan", "MathArcsin", "asin", "MathArccos", "acos", "MathMod", "fmod",
    "MathRand", "rand", "MathSrand", "srand", "MathIsValidNumber", "StringLen", "StringSubstr", "StringFind", "StringReplace",
    "StringToUpper", "StringToLower", "StringTrimLeft", "StringTrimRight", "StringSplit", "StringConcatenate", "StringCompare",
    "StringGetCharacter", "StringAdd", "ArraySize", "ArrayRange", "ArrayResize", "ArrayFree", "ArrayInitialize", "ArrayFill",
    "ArraySetAsSeries", "ArrayGetAsSeries", "ArrayIsSeries", "ArrayIsDynamic", "ArrayCopy", "ArrayMaximum", "ArrayMinimum",
    "ArraySort", "ArrayReverse", "TimeCurrent", "TimeLocal", "TimeGMT", "TimeTradeServer", "TimeToStruct", "StructToTime",
    "GetTickCount", "GetMicrosecondCount", "Bars", "iBars", "iClose", "iOpen", "iHigh", "iLow", "iTime", "iVolume",
    "iTickVolume", "iHighest", "iLowest", "iBarShift", "CopyRates", "CopyClose", "CopyOpen", "CopyHigh", "CopyLow", "CopyTime",
    "CopyTickVolume", "Symbol", "Period", "Point", "Digits", "SymbolInfoDouble", "SymbolInfoInteger", "SymbolInfoString",
    "SymbolInfoTick", "SymbolsTotal", "SymbolName", "SymbolSelect", "RefreshRates", "AccountInfoDouble", "AccountInfoInteger",
    "AccountInfoString", "OrderSend", "OrderCheck", "PositionsTotal", "PositionGetTicket", "PositionGetSymbol", "PositionSelect",
    "PositionSelectByTicket", "PositionGetDouble", "PositionGetInteger", "PositionGetString", "OrdersTotal", "OrderGetTicket",
    "OrderSelect", "OrderGetDouble", "OrderGetInteger", "OrderGetString", "HistorySelect", "HistoryDealsTotal",
    "HistoryDealGetTicket", "HistoryDealGetDouble", "HistoryDealGetInteger", "HistoryDealGetString", "HistoryOrdersTotal", "iMA",
    "iRSI", "iMACD", "iBands", "iStochastic", "iATR", "iCCI", "iMomentum", "iADX", "iEnvelopes", "iSAR", "iWPR", "iIchimoku",
    "iAlligator", "iFractals", "iVolumes", "iCustom", "IndicatorRelease", "CopyBuffer", "BarsCalculated", "SetIndexBuffer",
    "PlotIndexSetInteger", "PlotIndexSetDouble", "PlotIndexSetString", "PlotIndexGetInteger", "IndicatorSetString",
    "IndicatorSetInteger", "IndicatorSetDouble", "ObjectCreate", "ObjectDelete", "ObjectsDeleteAll", "ObjectFind",
    "ObjectSetInteger", "ObjectSetDouble", "ObjectSetString", "ObjectGetInteger", "ObjectGetDouble", "ObjectGetString",
    "ObjectMove", "ObjectsTotal", "ObjectName", "ChartRedraw", "ChartID", "ChartSymbol", "ChartPeriod", "ChartGetInteger",
    "ChartSetInteger", "ChartGetDouble", "EventSetTimer", "EventSetMillisecondTimer", "EventKillTimer", "GetLastError",
    "ResetLastError", "IsStopped", "ExpertRemove", "Sleep", "TerminalInfoInteger", "TerminalInfoString", "MQLInfoInteger",
    "MQLInfoString", "PlaySound", "SendNotification", "SendMail", "GlobalVariableSet", "GlobalVariableGet",
    "GlobalVariableCheck", "GlobalVariableDel", "ZeroMemory", "CheckPointer", "IsTradeAllowed", "TerminalClose", "MathSign",
];

pub fn is_builtin_func(name: &str) -> bool {
    static SET: OnceLock<std::collections::HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| BUILTIN_FUNCS.iter().copied().collect()).contains(name)
}

fn colors() -> Vec<(&'static str, i64)> {
    // values are BGR like the script language's `color` type
    vec![
        ("Black", 0x000000),
        ("White", 0xFFFFFF),
        ("Red", 0x0000FF),
        ("Lime", 0x00FF00),
        ("Blue", 0xFF0000),
        ("Yellow", 0x00FFFF),
        ("Aqua", 0xFFFF00),
        ("Magenta", 0xFF00FF),
        ("Green", 0x008000),
        ("Orange", 0x00A5FF),
        ("Gold", 0x00D7FF),
        ("Silver", 0xC0C0C0),
        ("Gray", 0x808080),
        ("DarkGray", 0xA9A9A9),
        ("DimGray", 0x696969),
        ("LightGray", 0xD3D3D3),
        ("DodgerBlue", 0xFF901E),
        ("DeepSkyBlue", 0xFFBF00),
        ("LightSeaGreen", 0xAAB220),
        ("MediumSeaGreen", 0x71B33C),
        ("SeaGreen", 0x578B2E),
        ("Tomato", 0x4763FF),
        ("Crimson", 0x3C14DC),
        ("OrangeRed", 0x0045FF),
        ("Navy", 0x800000),
        ("Purple", 0x800080),
        ("Teal", 0x808000),
        ("Maroon", 0x000080),
        ("Olive", 0x008080),
        ("Violet", 0xEE82EE),
        ("Pink", 0xCBC0FF),
        ("Brown", 0x2A2AA5),
        ("YellowGreen", 0x32CD9A),
        ("SpringGreen", 0x7FFF00),
        ("LimeGreen", 0x32CD32),
        ("ForestGreen", 0x228B22),
        ("DarkGreen", 0x006400),
        ("SteelBlue", 0xB48246),
        ("RoyalBlue", 0xE16941),
        ("SlateGray", 0x908070),
        ("LightSlateGray", 0x998877),
        ("Wheat", 0xB3DEF5),
        ("Khaki", 0x8CE6F0),
        ("SandyBrown", 0x60A4F4),
        ("IndianRed", 0x5C5CCD),
        ("FireBrick", 0x2222B2),
        ("DarkOrange", 0x008CFF),
        ("Chocolate", 0x1E69D2),
        ("Coral", 0x507FFF),
        ("Salmon", 0x7280FA),
        ("Turquoise", 0xD0E040),
        ("MediumBlue", 0xCD0000),
        ("DarkViolet", 0xD30094),
        ("Thistle", 0xD8BFD8),
        ("Lavender", 0xFAE6E6),
        ("SkyBlue", 0xEBCE87),
        ("Beige", 0xDCF5F5),
        ("DarkSlateGray", 0x4F4F2F),
    ]
}

fn table() -> &'static HashMap<String, Const> {
    static T: OnceLock<HashMap<String, Const>> = OnceLock::new();
    T.get_or_init(|| {
        use Const::*;
        let mut m: HashMap<String, Const> = HashMap::new();
        let mut add = |k: &str, v: Const| {
            m.insert(k.to_string(), v);
        };
        let ints: &[(&str, i64)] = &[
            // drawing
            ("DRAW_NONE", 0), ("DRAW_LINE", 1), ("DRAW_SECTION", 2), ("DRAW_HISTOGRAM", 3), ("DRAW_HISTOGRAM2", 4), ("DRAW_ARROW", 5),
            ("DRAW_ZIGZAG", 6), ("DRAW_FILLING", 7), ("DRAW_BARS", 8), ("DRAW_CANDLES", 9), ("DRAW_COLOR_LINE", 10),
            ("DRAW_COLOR_SECTION", 11), ("DRAW_COLOR_HISTOGRAM", 12), ("DRAW_COLOR_HISTOGRAM2", 13), ("DRAW_COLOR_ARROW", 14),
            ("DRAW_COLOR_ZIGZAG", 15), ("DRAW_COLOR_BARS", 16), ("DRAW_COLOR_CANDLES", 17),
            ("STYLE_SOLID", 0), ("STYLE_DASH", 1), ("STYLE_DOT", 2), ("STYLE_DASHDOT", 3), ("STYLE_DASHDOTDOT", 4),
            ("INDICATOR_DATA", 0), ("INDICATOR_COLOR_INDEX", 1), ("INDICATOR_CALCULATIONS", 2),
            ("PLOT_ARROW", 0), ("PLOT_ARROW_SHIFT", 1), ("PLOT_DRAW_BEGIN", 2), ("PLOT_DRAW_TYPE", 3), ("PLOT_SHOW_DATA", 4),
            ("PLOT_SHIFT", 5), ("PLOT_LINE_STYLE", 6), ("PLOT_LINE_WIDTH", 7), ("PLOT_COLOR_INDEXES", 8), ("PLOT_LINE_COLOR", 9),
            ("PLOT_EMPTY_VALUE", 10), ("PLOT_LABEL", 11),
            ("INDICATOR_SHORTNAME", 0), ("INDICATOR_DIGITS", 1), ("INDICATOR_HEIGHT", 2), ("INDICATOR_LEVELS", 3),
            ("INDICATOR_LEVELCOLOR", 4), ("INDICATOR_LEVELSTYLE", 5), ("INDICATOR_LEVELWIDTH", 6), ("INDICATOR_MINIMUM", 7),
            ("INDICATOR_MAXIMUM", 8), ("INDICATOR_LEVELVALUE", 9), ("INDICATOR_LEVELTEXT", 10),
            // MA methods and prices
            ("MODE_SMA", 0), ("MODE_EMA", 1), ("MODE_SMMA", 2), ("MODE_LWMA", 3),
            ("PRICE_CLOSE", 1), ("PRICE_OPEN", 2), ("PRICE_HIGH", 3), ("PRICE_LOW", 4), ("PRICE_MEDIAN", 5), ("PRICE_TYPICAL", 6),
            ("PRICE_WEIGHTED", 7),
            ("MAIN_LINE", 0), ("SIGNAL_LINE", 1), ("MODE_MAIN", 0), ("MODE_SIGNAL", 1), ("BASE_LINE", 0), ("UPPER_BAND", 1),
            ("LOWER_BAND", 2), ("PLUSDI_LINE", 1), ("MINUSDI_LINE", 2), ("UPPER_LINE", 0), ("LOWER_LINE", 1), ("TENKANSEN_LINE", 0),
            ("KIJUNSEN_LINE", 1), ("SENKOUSPANA_LINE", 2), ("SENKOUSPANB_LINE", 3), ("CHIKOUSPAN_LINE", 4), ("GATORJAW_LINE", 0),
            ("GATORTEETH_LINE", 1), ("GATORLIPS_LINE", 2), ("STO_LOWHIGH", 0), ("STO_CLOSECLOSE", 1), ("VOLUME_TICK", 0),
            ("VOLUME_REAL", 1),
            ("MODE_OPEN", 0), ("MODE_LOW", 1), ("MODE_HIGH", 2), ("MODE_CLOSE", 3), ("MODE_VOLUME", 4), ("MODE_REAL_VOLUME", 5),
            ("MODE_SPREAD", 6), ("MODE_TIME", 7),
            // timeframes
            ("PERIOD_CURRENT", 0), ("PERIOD_M1", 1), ("PERIOD_M2", 2), ("PERIOD_M3", 3), ("PERIOD_M4", 4), ("PERIOD_M5", 5),
            ("PERIOD_M10", 10), ("PERIOD_M15", 15), ("PERIOD_M30", 30), ("PERIOD_H1", 16385), ("PERIOD_H2", 16386),
            ("PERIOD_H4", 16388), ("PERIOD_D1", 16408), ("PERIOD_W1", 32769), ("PERIOD_MN1", 49153),
            // trading
            ("ORDER_TYPE_BUY", 0), ("ORDER_TYPE_SELL", 1), ("ORDER_TYPE_BUY_LIMIT", 2), ("ORDER_TYPE_SELL_LIMIT", 3),
            ("ORDER_TYPE_BUY_STOP", 4), ("ORDER_TYPE_SELL_STOP", 5), ("ORDER_TYPE_BUY_STOP_LIMIT", 6),
            ("ORDER_TYPE_SELL_STOP_LIMIT", 7), ("ORDER_TYPE_CLOSE_BY", 8),
            ("POSITION_TYPE_BUY", 0), ("POSITION_TYPE_SELL", 1),
            ("TRADE_ACTION_DEAL", 1), ("TRADE_ACTION_PENDING", 5), ("TRADE_ACTION_SLTP", 6), ("TRADE_ACTION_MODIFY", 7),
            ("TRADE_ACTION_REMOVE", 8), ("TRADE_ACTION_CLOSE_BY", 10),
            ("ORDER_FILLING_FOK", 0), ("ORDER_FILLING_IOC", 1), ("ORDER_FILLING_RETURN", 2), ("ORDER_FILLING_BOC", 3),
            ("ORDER_TIME_GTC", 0), ("ORDER_TIME_DAY", 1), ("ORDER_TIME_SPECIFIED", 2), ("ORDER_TIME_SPECIFIED_DAY", 3),
            ("TRADE_RETCODE_REQUOTE", 10004), ("TRADE_RETCODE_REJECT", 10006), ("TRADE_RETCODE_CANCEL", 10007),
            ("TRADE_RETCODE_PLACED", 10008), ("TRADE_RETCODE_DONE", 10009), ("TRADE_RETCODE_DONE_PARTIAL", 10010),
            ("TRADE_RETCODE_ERROR", 10011), ("TRADE_RETCODE_TIMEOUT", 10012), ("TRADE_RETCODE_INVALID", 10013),
            ("TRADE_RETCODE_INVALID_VOLUME", 10014), ("TRADE_RETCODE_INVALID_PRICE", 10015),
            ("TRADE_RETCODE_INVALID_STOPS", 10016), ("TRADE_RETCODE_TRADE_DISABLED", 10017),
            ("TRADE_RETCODE_MARKET_CLOSED", 10018), ("TRADE_RETCODE_NO_MONEY", 10019), ("TRADE_RETCODE_PRICE_CHANGED", 10020),
            ("TRADE_RETCODE_PRICE_OFF", 10021), ("TRADE_RETCODE_CLIENT_DISABLES_AT", 10027),
            ("TRADE_RETCODE_POSITION_CLOSED", 10036),
            ("SYMBOL_BID", 1), ("SYMBOL_ASK", 2), ("SYMBOL_POINT", 3), ("SYMBOL_DIGITS", 4), ("SYMBOL_SPREAD", 5),
            ("SYMBOL_VOLUME_MIN", 6), ("SYMBOL_VOLUME_MAX", 7), ("SYMBOL_VOLUME_STEP", 8), ("SYMBOL_TRADE_CONTRACT_SIZE", 9),
            ("SYMBOL_TRADE_TICK_VALUE", 10), ("SYMBOL_TRADE_TICK_SIZE", 11), ("SYMBOL_TRADE_STOPS_LEVEL", 12),
            ("SYMBOL_LAST", 13), ("SYMBOL_TIME", 14), ("SYMBOL_SPREAD_FLOAT", 15), ("SYMBOL_TRADE_FREEZE_LEVEL", 16),
            ("SYMBOL_DESCRIPTION", 17), ("SYMBOL_CURRENCY_BASE", 18), ("SYMBOL_CURRENCY_PROFIT", 19),
            ("SYMBOL_CURRENCY_MARGIN", 20), ("SYMBOL_PATH", 21), ("SYMBOL_BIDHIGH", 22), ("SYMBOL_BIDLOW", 23),
            ("SYMBOL_SELECT", 24), ("SYMBOL_VISIBLE", 25), ("SYMBOL_TRADE_MODE", 26), ("SYMBOL_FILLING_MODE", 27),
            ("SYMBOL_TRADE_TICK_VALUE_PROFIT", 28), ("SYMBOL_TRADE_TICK_VALUE_LOSS", 29), ("SYMBOL_SESSION_OPEN", 30),
            ("ACCOUNT_BALANCE", 1), ("ACCOUNT_EQUITY", 2), ("ACCOUNT_PROFIT", 3), ("ACCOUNT_MARGIN", 4),
            ("ACCOUNT_MARGIN_FREE", 5), ("ACCOUNT_FREEMARGIN", 5), ("ACCOUNT_MARGIN_LEVEL", 6), ("ACCOUNT_CREDIT", 7),
            ("ACCOUNT_MARGIN_SO_CALL", 8), ("ACCOUNT_MARGIN_SO_SO", 9), ("ACCOUNT_LOGIN", 10), ("ACCOUNT_LEVERAGE", 11),
            ("ACCOUNT_TRADE_ALLOWED", 12), ("ACCOUNT_TRADE_EXPERT", 13), ("ACCOUNT_LIMIT_ORDERS", 14), ("ACCOUNT_TRADE_MODE", 15),
            ("ACCOUNT_MARGIN_MODE", 16), ("ACCOUNT_NAME", 17), ("ACCOUNT_SERVER", 18), ("ACCOUNT_CURRENCY", 19),
            ("ACCOUNT_COMPANY", 20), ("ACCOUNT_TRADE_MODE_DEMO", 0), ("ACCOUNT_TRADE_MODE_REAL", 2),
            ("ACCOUNT_MARGIN_MODE_RETAIL_HEDGING", 2), ("ACCOUNT_MARGIN_MODE_RETAIL_NETTING", 0),
            ("POSITION_TICKET", 1), ("POSITION_TIME", 2), ("POSITION_TYPE", 3), ("POSITION_MAGIC", 4), ("POSITION_IDENTIFIER", 5),
            ("POSITION_VOLUME", 6), ("POSITION_PRICE_OPEN", 7), ("POSITION_SL", 8), ("POSITION_TP", 9),
            ("POSITION_PRICE_CURRENT", 10), ("POSITION_PROFIT", 11), ("POSITION_SWAP", 12), ("POSITION_SYMBOL", 13),
            ("POSITION_COMMENT", 14), ("POSITION_TIME_MSC", 15), ("POSITION_REASON", 16),
            ("ORDER_TICKET", 1), ("ORDER_TYPE", 2), ("ORDER_MAGIC", 3), ("ORDER_TIME_SETUP", 4), ("ORDER_VOLUME_CURRENT", 5),
            ("ORDER_VOLUME_INITIAL", 6), ("ORDER_PRICE_OPEN", 7), ("ORDER_SL", 8), ("ORDER_TP", 9), ("ORDER_SYMBOL", 10),
            ("ORDER_COMMENT", 11), ("ORDER_PRICE_CURRENT", 12), ("ORDER_STATE", 13),
            ("DEAL_TICKET", 1), ("DEAL_ORDER", 2), ("DEAL_TIME", 3), ("DEAL_TYPE", 4), ("DEAL_ENTRY", 5), ("DEAL_MAGIC", 6),
            ("DEAL_POSITION_ID", 7), ("DEAL_VOLUME", 8), ("DEAL_PRICE", 9), ("DEAL_PROFIT", 10), ("DEAL_SYMBOL", 11),
            ("DEAL_COMMENT", 12), ("DEAL_COMMISSION", 13), ("DEAL_SWAP", 14),
            ("DEAL_ENTRY_IN", 0), ("DEAL_ENTRY_OUT", 1), ("DEAL_ENTRY_INOUT", 2), ("DEAL_TYPE_BUY", 0), ("DEAL_TYPE_SELL", 1),
            ("DEAL_TYPE_BALANCE", 2),
            ("INIT_SUCCEEDED", 0), ("INIT_FAILED", 1), ("INIT_PARAMETERS_INCORRECT", 32767), ("INIT_AGENT_NOT_SUITABLE", 32766),
            ("REASON_PROGRAM", 0), ("REASON_REMOVE", 1), ("REASON_RECOMPILE", 2), ("REASON_CHARTCHANGE", 3),
            ("REASON_CHARTCLOSE", 4), ("REASON_PARAMETERS", 5), ("REASON_ACCOUNT", 6), ("REASON_TEMPLATE", 7),
            ("REASON_INITFAILED", 8), ("REASON_CLOSE", 9),
            ("EMPTY", -1), ("WRONG_VALUE", -1), ("INVALID_HANDLE", -1), ("WHOLE_ARRAY", -1), ("CLR_NONE", -1), ("clrNONE", -1),
            ("INT_MAX", i32::MAX as i64), ("INT_MIN", i32::MIN as i64), ("UINT_MAX", u32::MAX as i64), ("LONG_MAX", i64::MAX),
            ("LONG_MIN", i64::MIN), ("SHORT_MAX", 32767), ("CHAR_MAX", 127), ("UCHAR_MAX", 255), ("CHARTS_MAX", 100),
            ("TIME_DATE", 1), ("TIME_MINUTES", 2), ("TIME_SECONDS", 4),
            // objects
            ("OBJ_VLINE", 0), ("OBJ_HLINE", 1), ("OBJ_TREND", 2), ("OBJ_RECTANGLE", 16), ("OBJ_TEXT", 21), ("OBJ_ARROW", 22),
            ("OBJ_LABEL", 23), ("OBJ_BUTTON", 25), ("OBJ_FIBO", 10), ("OBJ_ARROW_UP", 31), ("OBJ_ARROW_DOWN", 32),
            ("OBJ_ARROW_BUY", 34), ("OBJ_ARROW_SELL", 35), ("OBJ_RECTANGLE_LABEL", 110), ("OBJ_CHANNEL", 5),
            ("OBJ_ARROWED_LINE", 108), ("OBJ_TRIANGLE", 17), ("OBJ_ELLIPSE", 18),
            ("OBJPROP_COLOR", 0), ("OBJPROP_STYLE", 1), ("OBJPROP_WIDTH", 2), ("OBJPROP_BACK", 3), ("OBJPROP_SELECTABLE", 4),
            ("OBJPROP_SELECTED", 5), ("OBJPROP_HIDDEN", 6), ("OBJPROP_TIME", 7), ("OBJPROP_PRICE", 8), ("OBJPROP_TEXT", 9),
            ("OBJPROP_FONT", 10), ("OBJPROP_FONTSIZE", 11), ("OBJPROP_XDISTANCE", 12), ("OBJPROP_YDISTANCE", 13),
            ("OBJPROP_CORNER", 14), ("OBJPROP_ANCHOR", 15), ("OBJPROP_ARROWCODE", 16), ("OBJPROP_RAY_RIGHT", 17),
            ("OBJPROP_RAY_LEFT", 18), ("OBJPROP_XSIZE", 19), ("OBJPROP_YSIZE", 20), ("OBJPROP_BGCOLOR", 21),
            ("OBJPROP_FILL", 22), ("OBJPROP_ZORDER", 23), ("OBJPROP_STATE", 24), ("OBJPROP_TOOLTIP", 25), ("OBJPROP_TYPE", 26),
            ("OBJPROP_NAME", 27), ("OBJPROP_BORDER_COLOR", 28), ("OBJPROP_ANGLE", 29), ("OBJPROP_LEVELS", 30),
            ("CORNER_LEFT_UPPER", 0), ("CORNER_RIGHT_UPPER", 1), ("CORNER_LEFT_LOWER", 2), ("CORNER_RIGHT_LOWER", 3),
            ("ANCHOR_LEFT_UPPER", 0), ("ANCHOR_LEFT", 1), ("ANCHOR_LEFT_LOWER", 2), ("ANCHOR_LOWER", 3),
            ("ANCHOR_RIGHT_LOWER", 4), ("ANCHOR_RIGHT", 5), ("ANCHOR_RIGHT_UPPER", 6), ("ANCHOR_UPPER", 7), ("ANCHOR_CENTER", 8),
            ("ANCHOR_TOP", 0), ("ANCHOR_BOTTOM", 1),
            ("CHART_WIDTH_IN_PIXELS", 1), ("CHART_HEIGHT_IN_PIXELS", 2), ("CHART_FIRST_VISIBLE_BAR", 3),
            ("CHART_VISIBLE_BARS", 4), ("CHART_SHOW_GRID", 5), ("CHART_MODE", 6), ("CHART_AUTOSCROLL", 7), ("CHART_SHIFT", 8),
            ("CHART_PRICE_MAX", 9), ("CHART_PRICE_MIN", 10), ("CHART_BARS", 0), ("CHART_CANDLES", 1), ("CHART_LINE", 2),
            ("CHART_COLOR_BACKGROUND", 11), ("CHART_COLOR_FOREGROUND", 12),
            ("TERMINAL_TRADE_ALLOWED", 1), ("TERMINAL_CONNECTED", 2), ("TERMINAL_BUILD", 3), ("TERMINAL_DLLS_ALLOWED", 4),
            ("TERMINAL_MAXBARS", 5), ("TERMINAL_PATH", 6), ("TERMINAL_DATA_PATH", 7), ("TERMINAL_LANGUAGE", 8),
            ("TERMINAL_NAME", 9), ("TERMINAL_COMPANY", 10), ("TERMINAL_PING_LAST", 11),
            ("MQL_TRADE_ALLOWED", 1), ("MQL_TESTER", 2), ("MQL_OPTIMIZATION", 3), ("MQL_VISUAL_MODE", 4), ("MQL_DEBUG", 5),
            ("MQL_PROGRAM_NAME", 6), ("MQL_PROGRAM_TYPE", 7), ("MQL_DLLS_ALLOWED", 8), ("MQL_PROGRAM_PATH", 9),
            ("POINTER_INVALID", 0), ("POINTER_DYNAMIC", 1), ("POINTER_AUTOMATIC", 2),
            ("ERR_SUCCESS", 0), ("ERR_NO_ERROR", 0), ("ERR_INVALID_PARAMETER", 4003), ("ERR_ARRAY_RESIZE_ERROR", 4007),
            ("ERR_INDICATOR_DATA_NOT_FOUND", 4806), ("ERR_INDICATOR_WRONG_HANDLE", 4807), ("ERR_TRADE_DISABLED", 4109),
            ("ERR_MARKET_UNKNOWN_SYMBOL", 4301), ("ERR_TRADE_POSITION_NOT_FOUND", 4753), ("ERR_TRADE_ORDER_NOT_FOUND", 4754),
            ("ERR_TRADE_SEND_FAILED", 4756), ("ERR_USER_ERROR_FIRST", 65536),
            ("CHARTEVENT_KEYDOWN", 0), ("CHARTEVENT_MOUSE_MOVE", 10), ("CHARTEVENT_OBJECT_CREATE", 7),
            ("CHARTEVENT_OBJECT_CHANGE", 8), ("CHARTEVENT_OBJECT_DELETE", 6), ("CHARTEVENT_CLICK", 4),
            ("CHARTEVENT_OBJECT_CLICK", 1), ("CHARTEVENT_OBJECT_DRAG", 2), ("CHARTEVENT_OBJECT_ENDEDIT", 3),
            ("CHARTEVENT_CHART_CHANGE", 9), ("CHARTEVENT_CUSTOM", 1000),
        ];
        for (k, v) in ints {
            add(k, I(*v));
        }
        for (k, v) in colors() {
            add(&format!("clr{}", k), I(v));
            add(k, I(v));
        }
        let dbls: &[(&str, f64)] = &[
            ("EMPTY_VALUE", f64::MAX),
            ("DBL_MAX", f64::MAX),
            ("DBL_MIN", f64::MIN_POSITIVE),
            ("DBL_EPSILON", f64::EPSILON),
            ("FLT_MAX", f32::MAX as f64),
            ("FLT_MIN", f32::MIN_POSITIVE as f64),
            ("M_PI", std::f64::consts::PI),
            ("M_PI_2", std::f64::consts::FRAC_PI_2),
            ("M_E", std::f64::consts::E),
            ("M_SQRT2", std::f64::consts::SQRT_2),
            ("M_LN2", std::f64::consts::LN_2),
            ("M_LN10", std::f64::consts::LN_10),
        ];
        for (k, v) in dbls {
            add(k, D(*v));
        }
        m
    })
}

pub fn constant(name: &str) -> Option<Const> {
    table().get(name).copied()
}

pub fn color_names() -> Vec<(&'static str, i64)> {
    colors()
}

/// MQL enum names known for input drop-downs.
pub fn builtin_enum(name: &str) -> Option<Vec<(&'static str, i64)>> {
    Some(match name {
        "ENUM_MA_METHOD" => vec![("Simple", 0), ("Exponential", 1), ("Smoothed", 2), ("Linear weighted", 3)],
        "ENUM_APPLIED_PRICE" => vec![
            ("Close price", 1),
            ("Open price", 2),
            ("High price", 3),
            ("Low price", 4),
            ("Median price (HL/2)", 5),
            ("Typical price (HLC/3)", 6),
            ("Weighted price (HLCC/4)", 7),
        ],
        "ENUM_TIMEFRAMES" => vec![
            ("current", 0),
            ("1 Minute", 1),
            ("5 Minutes", 5),
            ("15 Minutes", 15),
            ("30 Minutes", 30),
            ("1 Hour", 16385),
            ("4 Hours", 16388),
            ("1 Day", 16408),
            ("1 Week", 32769),
            ("1 Month", 49153),
        ],
        "ENUM_STO_PRICE" => vec![("Low/High", 0), ("Close/Close", 1)],
        "ENUM_APPLIED_VOLUME" => vec![("Tick volume", 0), ("Real volume", 1)],
        "ENUM_LINE_STYLE" => vec![("Solid", 0), ("Dash", 1), ("Dot", 2), ("Dash-dot", 3), ("Dash-dot-dot", 4)],
        _ => return None,
    })
}
