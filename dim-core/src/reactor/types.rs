#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Table {
    Library,
    Media,
    Assets,
}

impl TryFrom<&str> for Table {
    type Error = ();

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "library" => Ok(Self::Library),
            "_tblmedia" => Ok(Self::Media),
            "assets" => Ok(Self::Assets),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum EventType {
    Insert,
    Update,
    Delete,
}

impl TryFrom<&str> for EventType {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "insert" => Ok(Self::Insert),
            "update" => Ok(Self::Update),
            "delete" => Ok(Self::Delete),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub id: i64,
    pub event_type: EventType,
    pub table: Table,
}
