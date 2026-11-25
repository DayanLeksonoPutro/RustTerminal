//! Chart objects: lines, trend lines, rectangles, Fibonacci levels, text and labels. Created
//! by the drawing tools or by scripts (ObjectCreate).

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ObjKind {
    VLine,
    HLine,
    Trend,
    Rectangle,
    Fibo,
    Text,
    Label,
    ArrowUp,
    ArrowDown,
    ArrowBuy,
    ArrowSell,
    Button,
    RectLabel,
}

impl ObjKind {
    pub fn from_mql(v: i64) -> Option<ObjKind> {
        Some(match v {
            0 => ObjKind::VLine,
            1 => ObjKind::HLine,
            2 | 108 | 5 => ObjKind::Trend,
            16 | 17 | 18 => ObjKind::Rectangle,
            10 => ObjKind::Fibo,
            21 => ObjKind::Text,
            22 | 31 => ObjKind::ArrowUp,
            32 => ObjKind::ArrowDown,
            34 => ObjKind::ArrowBuy,
            35 => ObjKind::ArrowSell,
            23 => ObjKind::Label,
            25 => ObjKind::Button,
            110 => ObjKind::RectLabel,
            _ => return None,
        })
    }
    pub fn mql(self) -> i64 {
        match self {
            ObjKind::VLine => 0,
            ObjKind::HLine => 1,
            ObjKind::Trend => 2,
            ObjKind::Rectangle => 16,
            ObjKind::Fibo => 10,
            ObjKind::Text => 21,
            ObjKind::Label => 23,
            ObjKind::ArrowUp => 31,
            ObjKind::ArrowDown => 32,
            ObjKind::ArrowBuy => 34,
            ObjKind::ArrowSell => 35,
            ObjKind::Button => 25,
            ObjKind::RectLabel => 110,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ObjKind::VLine => "Vertical Line",
            ObjKind::HLine => "Horizontal Line",
            ObjKind::Trend => "Trend Line",
            ObjKind::Rectangle => "Rectangle",
            ObjKind::Fibo => "Fibonacci Retracement",
            ObjKind::Text => "Text",
            ObjKind::Label => "Label",
            ObjKind::ArrowUp => "Arrow Up",
            ObjKind::ArrowDown => "Arrow Down",
            ObjKind::ArrowBuy => "Buy",
            ObjKind::ArrowSell => "Sell",
            ObjKind::Button => "Button",
            ObjKind::RectLabel => "Rectangle Label",
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ChartObj {
    pub name: String,
    pub kind: ObjKind,
    pub times: [i64; 3],
    pub prices: [f64; 3],
    /// BGR colour like the script language
    pub color: u32,
    pub bgcolor: u32,
    pub width: i64,
    pub style: i64,
    pub text: String,
    pub font_size: i64,
    pub corner: i64,
    pub xdist: i64,
    pub ydist: i64,
    pub xsize: i64,
    pub ysize: i64,
    pub ray_right: bool,
    pub back: bool,
    pub selectable: bool,
    pub hidden: bool,
    pub arrow_code: i64,
    pub anchor: i64,
    pub tooltip: String,
    pub state: bool,
    /// created by a program (removed with it)
    pub from_program: bool,
}

impl ChartObj {
    pub fn new(name: &str, kind: ObjKind) -> Self {
        let color = match kind {
            ObjKind::Label | ObjKind::Text => 0xFFFFFF,
            ObjKind::Fibo => 0x00D7FF,
            ObjKind::ArrowBuy => 0xFF901E,
            ObjKind::ArrowSell => 0x0000FF,
            _ => 0x0000FF,
        };
        ChartObj {
            name: name.to_string(),
            kind,
            times: [0; 3],
            prices: [0.0; 3],
            color,
            bgcolor: 0x404040,
            width: 1,
            style: 0,
            text: String::new(),
            font_size: 10,
            corner: 0,
            xdist: 0,
            ydist: 0,
            xsize: 100,
            ysize: 20,
            ray_right: false,
            back: false,
            selectable: true,
            hidden: false,
            arrow_code: 0,
            anchor: 0,
            tooltip: String::new(),
            state: false,
            from_program: false,
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ChartObjects {
    pub list: Vec<ChartObj>,
}

impl ChartObjects {
    pub fn find(&self, name: &str) -> Option<&ChartObj> {
        self.list.iter().find(|o| o.name == name)
    }
    pub fn find_mut(&mut self, name: &str) -> Option<&mut ChartObj> {
        self.list.iter_mut().find(|o| o.name == name)
    }
    pub fn delete(&mut self, name: &str) -> bool {
        let n = self.list.len();
        self.list.retain(|o| o.name != name);
        n != self.list.len()
    }
    pub fn unique_name(&self, base: &str) -> String {
        let mut i = 1;
        loop {
            let n = format!("{} {}", base, i);
            if self.find(&n).is_none() {
                return n;
            }
            i += 1;
        }
    }
}

pub const FIBO_LEVELS: [f64; 7] = [0.0, 0.236, 0.382, 0.5, 0.618, 1.0, 1.618];
