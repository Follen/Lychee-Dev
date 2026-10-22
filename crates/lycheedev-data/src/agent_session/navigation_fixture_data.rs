//! Constructor copies from unchanged mature fixture sources; new recovery candidate.
// item_models_parity.rs SHA256 16afb396755b4a9a53ff3688d2f1ff8e6f10b9838b97d738310d8eb6fba21e50
#[allow(unused_imports, dead_code)]
pub(super) mod models {
    use crate::capture::fixture::{
        Cell,
        Cell::{Array, F, I, I64, Text, U},
        Fixture, Table, table,
    };
    use serde_json::{Value, json};
    fn array(a: u32, b: u32, c: u32) -> Cell {
        Array(vec![U(a), U(b), U(c)])
    }
    fn modified(rows: &[Vec<Cell>]) -> Table {
        table(
            "ItemModifiedAppearance",
            502,
            &[
                ("ID", "uint"),
                ("ItemID", "uint"),
                ("ItemAppearanceID", "uint"),
            ],
            rows,
        )
    }
    fn models(rows: &[Vec<Cell>]) -> Table {
        table(
            "ModelFileData",
            505,
            &[
                ("ID", "uint"),
                ("ModelResourcesID", "uint"),
                ("FileDataID", "uint"),
            ],
            rows,
        )
    }
    fn displays(rows: &[Vec<Cell>]) -> Table {
        table(
            "ItemDisplayInfo",
            504,
            &[
                ("ID", "uint"),
                ("ModelResourcesID", "array3"),
                ("ModelMaterialResourcesID", "array3"),
                ("GeosetGroup", "array2signed"),
            ],
            rows,
        )
    }
    pub(crate) fn tables() -> Vec<Table> {
        vec![
            table(
                "ItemSparse",
                501,
                &[("ID", "uint")],
                &(1..=6).map(|id| vec![U(id)]).collect::<Vec<_>>(),
            ),
            modified(&[
                vec![U(10), U(1), U(100)],
                vec![U(30), U(2), U(100)],
                vec![U(20), U(2), U(200)],
                vec![U(40), U(3), U(300)],
                vec![U(60), U(5), U(999)],
                vec![U(50), U(5), U(100)],
                vec![U(70), U(6), U(600)],
            ]),
            table(
                "ItemAppearance",
                503,
                &[("ID", "uint"), ("ItemDisplayInfoID", "uint")],
                &[
                    vec![U(100), U(1000)],
                    vec![U(300), U(999)],
                    vec![U(600), U(6000)],
                ],
            ),
            displays(&[
                vec![
                    U(1000),
                    array(50, 60, 50),
                    array(70, 80, 70),
                    Array(vec![I(-1), I(0)]),
                ],
                vec![
                    U(6000),
                    array(99, 0, 0),
                    array(99, 0, 0),
                    Array(vec![I(0), I(0)]),
                ],
            ]),
            models(&[
                vec![U(8), U(50), U(508)],
                vec![U(0), U(50), U(0)],
                vec![U(2), U(50), U(502)],
                vec![U(3), U(60), U(0)],
            ]),
            table(
                "TextureFileData",
                506,
                &[
                    ("ID", "uint"),
                    ("MaterialResourcesID", "uint"),
                    ("FileDataID", "uint"),
                    ("UsageType", "signed"),
                ],
                &[
                    vec![U(5), U(70), U(705), I(0)],
                    vec![U(0), U(70), U(0), I(0)],
                    vec![U(1), U(70), U(701), I(1)],
                    vec![U(4), U(70), U(0), I(0)],
                    vec![U(6), U(80), U(806), I(1)],
                ],
            ),
            table(
                "ComponentModelFileData",
                507,
                &[
                    ("ID", "uint"),
                    ("RaceID", "signed"),
                    ("GenderIndex", "signed"),
                ],
                &[
                    vec![U(502), I(1), I(0)],
                    vec![U(508), I(2), I(1)],
                    vec![U(3), I(2), I(1)],
                ],
            ),
        ]
    }
}
// item_textures_parity.rs SHA256 54e12894a96c70425e9c94824535ffdbdbd494f5b57ba04e7249814b9dd8e2e0
#[allow(unused_imports, dead_code)]
pub(super) mod textures {
    use crate::capture::fixture::{
        Cell,
        Cell::{Array, F, I, I64, Text, U},
        Fixture, Table, table,
    };
    use serde_json::{Value, json};
    fn sections(rows: &[Vec<Cell>]) -> Table {
        table(
            "ItemDisplayInfoMaterialRes",
            705,
            &[
                ("ID", "uint"),
                ("ItemDisplayInfoID", "uint"),
                ("MaterialResourcesID", "signed"),
                ("ComponentSection", "signed"),
            ],
            rows,
        )
    }
    fn textures(rows: &[Vec<Cell>]) -> Table {
        table(
            "TextureFileData",
            706,
            &[
                ("ID", "uint"),
                ("MaterialResourcesID", "uint"),
                ("FileDataID", "uint"),
                ("UsageType", "signed"),
            ],
            rows,
        )
    }
    pub(crate) fn tables() -> Vec<Table> {
        vec![
            table(
                "ItemSparse",
                701,
                &[("ID", "uint")],
                &(1..=7).map(|id| vec![U(id)]).collect::<Vec<_>>(),
            ),
            table(
                "ItemModifiedAppearance",
                702,
                &[
                    ("ID", "uint"),
                    ("ItemID", "uint"),
                    ("ItemAppearanceID", "uint"),
                ],
                &[
                    vec![U(10), U(1), U(100)],
                    vec![U(30), U(2), U(100)],
                    vec![U(20), U(2), U(200)],
                    vec![U(40), U(3), U(300)],
                    vec![U(60), U(5), U(999)],
                    vec![U(50), U(5), U(100)],
                    vec![U(70), U(6), U(600)],
                    vec![U(80), U(7), U(700)],
                ],
            ),
            table(
                "ItemAppearance",
                703,
                &[("ID", "uint"), ("ItemDisplayInfoID", "uint")],
                &[
                    vec![U(100), U(1000)],
                    vec![U(300), U(999)],
                    vec![U(600), U(6000)],
                    vec![U(700), U(0)],
                ],
            ),
            table(
                "ItemDisplayInfo",
                704,
                &[("ID", "uint")],
                &[vec![U(1000)], vec![U(6000)], vec![U(0)]],
            ),
            sections(&[
                vec![U(9), U(1000), I(70), I(-9)],
                vec![U(2), U(1000), I(70), I(-2)],
                vec![U(3), U(1000), I(0), I(3)],
                vec![U(4), U(1000), I(80), I(0)],
                vec![U(5), U(1000), I(90), I(5)],
                vec![U(6), U(1000), I(99), I(6)],
                vec![U(7), U(1000), I(-1), I(7)],
                vec![U(8), U(1000), I(70), I(0)],
                vec![U(10), U(0), I(70), I(10)],
            ]),
            textures(&[
                vec![U(5), U(70), U(705), I(0)],
                vec![U(0), U(70), U(0), I(0)],
                vec![U(1), U(70), U(701), I(1)],
                vec![U(4), U(70), U(0), I(0)],
                vec![U(6), U(80), U(806), I(1)],
                vec![U(7), U(80), U(807), I(-1)],
                vec![U(8), U(90), U(908), I(1)],
            ]),
        ]
    }
}
// item_geosets_parity.rs SHA256 96174fdcf73676e68f94e45e63fb67431e04a81bc30f0894cefdc3f69e9dcbe9
#[allow(unused_imports, dead_code)]
pub(super) mod geosets {
    use crate::capture::fixture::{
        Cell,
        Cell::{Array, F, I, I64, Text, U},
        Fixture, Table, table,
    };
    use serde_json::{Value, json};
    fn array(a: u32, b: u32, c: u32) -> Cell {
        Array(vec![U(a), U(b), U(c)])
    }
    fn displays(rows: &[Vec<Cell>]) -> Table {
        table(
            "ItemDisplayInfo",
            604,
            &[
                ("ID", "uint"),
                ("GeosetGroup", "array2signed"),
                ("HelmetGeosetVis", "array3"),
            ],
            rows,
        )
    }
    fn helmets(rows: &[Vec<Cell>]) -> Table {
        table(
            "HelmetGeosetData",
            605,
            &[
                ("ID", "uint"),
                ("HelmetGeosetVisDataID", "uint"),
                ("HideGeosetGroup", "signed"),
            ],
            rows,
        )
    }
    pub(crate) fn tables() -> Vec<Table> {
        vec![
            table(
                "ItemSparse",
                601,
                &[("ID", "uint")],
                &(1..=6).map(|id| vec![U(id)]).collect::<Vec<_>>(),
            ),
            table(
                "ItemModifiedAppearance",
                602,
                &[
                    ("ID", "uint"),
                    ("ItemID", "uint"),
                    ("ItemAppearanceID", "uint"),
                ],
                &[
                    vec![U(10), U(1), U(100)],
                    vec![U(30), U(2), U(100)],
                    vec![U(20), U(2), U(200)],
                    vec![U(40), U(3), U(300)],
                    vec![U(60), U(5), U(999)],
                    vec![U(50), U(5), U(100)],
                    vec![U(70), U(6), U(600)],
                ],
            ),
            table(
                "ItemAppearance",
                603,
                &[("ID", "uint"), ("ItemDisplayInfoID", "uint")],
                &[
                    vec![U(100), U(1000)],
                    vec![U(300), U(999)],
                    vec![U(600), U(6000)],
                ],
            ),
            displays(&[
                vec![U(1000), Array(vec![I(-1), I(0)]), array(10, 20, 10)],
                vec![U(6000), Array(vec![I(0), I(0)]), array(99, 0, 99)],
            ]),
            helmets(&[
                vec![U(8), U(10), I(80)],
                vec![U(2), U(10), I(-2)],
                vec![U(5), U(10), I(0)],
                vec![U(3), U(20), I(30)],
            ]),
        ]
    }
}
// spell_info_parity.rs SHA256 e76998eea35d096871dd8e62b566003f3f4522c61aeea8912d1c271f9ff3c4f0
#[allow(unused_imports, dead_code)]
pub(super) mod spell {
    use crate::capture::fixture::{
        Cell,
        Cell::{Array, F, I, I64, Text, U},
        Fixture, Table, table,
    };
    use serde_json::{Value, json};
    fn ints(a: i32, b: i32) -> Cell {
        Array(vec![I(a), I(b)])
    }
    fn effect(id: u32, spell: u32, kind: i32, trigger: u32, misc: i32) -> Vec<Cell> {
        vec![
            U(id),
            U(spell),
            U(0),
            I(kind),
            I(0),
            U(trigger),
            U(0),
            F(0.0),
            I(0),
            ints(6, 0),
            ints(0, 0),
            ints(misc, 0),
        ]
    }
    fn effects(rows: &[Vec<Cell>]) -> Table {
        table(
            "SpellEffect",
            303,
            &[
                ("ID", "uint"),
                ("SpellID", "uint"),
                ("EffectIndex", "uint"),
                ("Effect", "signed"),
                ("EffectAura", "signed"),
                ("EffectTriggerSpell", "uint"),
                ("EffectAuraPeriod", "uint"),
                ("EffectBasePointsF", "float"),
                ("EffectMechanic", "signed"),
                ("ImplicitTarget", "array2signed"),
                ("EffectRadiusIndex", "array2signed"),
                ("EffectMiscValue", "array2signed"),
            ],
            rows,
        )
    }
    fn names(rows: &[Vec<Cell>]) -> Table {
        table(
            "SpellName",
            301,
            &[("ID", "uint"), ("Name_lang", "text")],
            rows,
        )
    }
    fn spells(rows: &[Vec<Cell>]) -> Table {
        table(
            "Spell",
            302,
            &[
                ("ID", "uint"),
                ("Description_lang", "text"),
                ("AuraDescription_lang", "text"),
            ],
            rows,
        )
    }
    pub(crate) fn tables() -> Vec<Table> {
        let mut stun = effect(3, 7922, 6, 0, 0);
        stun[4] = I(12);
        stun[6] = U(180000);
        stun[7] = F(-1000.5);
        let mut trigger = effect(2, 100, 64, 7922, 0);
        trigger[2] = U(2);
        vec![
        names(&[vec![U(100),Text("Charge<&>🙂".into())],vec![U(7922),Text("Charge Stun".into())],vec![U(10),Text("A".into())],vec![U(20),Text("B".into())]]),
        spells(&[
            vec![U(100),Text("Charge an enemy, generate $/10;s2 rage, and stun it for $7922d.".into()),Text("".into())],
            vec![U(7922),Text("".into()),Text("Stunned.".into())],
            vec![U(10),Text("see $12345x and $@spellname77; twice $@spellname77; self $@spellname10; not $1234d or $4294967296x".into()),Text("$@spellname77".into())],
            vec![U(20),Text("".into()),Text("from $@spellname10".into())],
            vec![U(30),Text("self $@spellname30".into()),Text("".into())],
        ]),
        effects(&[trigger,effect(1,100,96,0,0),stun,effect(4,10,64,20,20),effect(5,20,64,10,0),effect(6,30,64,30,30),effect(7,40,64,10,20),effect(8,50,6,0,77)]),
        table("SpellMisc",304,&[("ID","uint"),("SpellID","uint"),("Attributes","array3"),("SchoolMask","signed"),("Speed","signed"),("SpellIconFileDataID","signed"),("CastingTimeIndex","uint"),("DurationIndex","uint"),("RangeIndex","uint")],&[
            vec![U(3),U(100),Array(vec![U(1),U(2),U(3)]),I(7),I(9),I(999),U(0),U(0),U(0)],
            vec![U(1),U(100),Array(vec![U(805634064),U(1024),U(0)]),I(1),I(0),I(132337),U(1),U(0),U(1)],
            vec![U(2),U(7922),Array(vec![U(327696),U(512),U(0)]),I(1),I(0),I(135860),U(1),U(2),U(1)],
        ]),
        table("SpellCastTimes",305,&[("ID","uint"),("Base","signed"),("Minimum","signed")],&[vec![U(1),I(0),I(0)]]),
        table("SpellDuration",306,&[("ID","uint"),("Duration","signed"),("MaxDuration","signed")],&[vec![U(2),I(1000),I(1000)]]),
        table("SpellRange",307,&[("ID","uint"),("DisplayName_lang","text"),("RangeMin","array2signed"),("RangeMax","array2signed")],&[vec![U(1),Text("Charge".into()),ints(8,8),ints(25,25)]]),
    ]
    }
}
// creature_navigation_parity.rs SHA256 ece31da1128eab50a6fcf33861d65229527923503ba48ecde31780b3ff8407bd
#[allow(unused_imports, dead_code)]
pub(super) mod creature {
    use crate::capture::fixture::{
        Cell,
        Cell::{Array, F, I, I64, Text, U},
        Fixture, Table, table,
    };
    use serde_json::{Value, json};
    pub(crate) fn tables() -> Vec<Table> {
        vec![
            table(
                "CreatureDisplayInfo",
                321,
                &[
                    ("ID", "uint"),
                    ("ModelID", "uint"),
                    ("TextureVariationFileDataID", "array3"),
                ],
                &[
                    vec![U(30), U(2), Array(vec![U(700), U(0), U(701)])],
                    vec![U(10), U(2), Array(vec![U(800), U(0), U(800)])],
                    vec![U(20), U(3), Array(vec![U(900), U(0), U(0)])],
                    vec![U(40), U(99), Array(vec![U(0), U(0), U(0)])],
                    vec![U(50), U(0), Array(vec![U(0), U(0), U(0)])],
                ],
            ),
            table(
                "CreatureModelData",
                322,
                &[("ID", "uint"), ("FileDataID", "uint")],
                &[vec![U(3), U(500)], vec![U(2), U(500)], vec![U(4), U(600)]],
            ),
            table(
                "CreatureDisplayInfoGeosetData",
                323,
                &[
                    ("ID", "uint"),
                    ("CreatureDisplayInfoID", "uint"),
                    ("GeosetIndex", "signed"),
                    ("GeosetValue", "signed"),
                ],
                &[
                    vec![U(3), U(10), I(2), I(5)],
                    vec![U(1), U(10), I(0), I(4)],
                    vec![U(2), U(30), I(1), I(7)],
                ],
            ),
        ]
    }
}
// encounter_navigation_parity.rs SHA256 4f393ab885b24134f3a989b71a4d52b400b1483145e13039c313821351500065
#[allow(unused_imports, dead_code)]
pub(super) mod encounter {
    use crate::capture::fixture::{
        Cell,
        Cell::{Array, F, I, I64, Text, U},
        Fixture, Table, table,
    };
    use serde_json::{Value, json};
    pub(crate) fn section(
        id: u32,
        order: u32,
        parent: u32,
        first: u32,
        next: u32,
        spell: u32,
    ) -> Vec<Cell> {
        vec![
            U(id),
            U(132),
            Text(format!("节{id}🙂")),
            Text("$74986A1\r\n正文".into()),
            U(spell),
            I(2),
            I(1),
            I(-1),
            U(0),
            U(order),
            U(parent),
            U(first),
            U(next),
        ]
    }
    fn sections(rows: &[Vec<Cell>]) -> Table {
        table(
            "JournalEncounterSection",
            331,
            &[
                ("ID", "uint"),
                ("JournalEncounterID", "uint"),
                ("Title_lang", "text"),
                ("BodyText_lang", "text"),
                ("SpellID", "uint"),
                ("IconFlags", "signed"),
                ("Type", "signed"),
                ("DifficultyMask", "signed"),
                ("IconCreatureDisplayInfoID", "uint"),
                ("OrderIndex", "uint"),
                ("ParentSectionID", "uint"),
                ("FirstChildSectionID", "uint"),
                ("NextSiblingSectionID", "uint"),
            ],
            rows,
        )
    }
    fn spell_tables() -> Vec<Table> {
        vec![
            table(
                "SpellName",
                332,
                &[("ID", "uint"), ("Name_lang", "text")],
                &[
                    vec![U(123), Text("Personal Phalanx".into())],
                    vec![U(456), Text("".into())],
                ],
            ),
            table(
                "SpellMisc",
                333,
                &[
                    ("ID", "uint"),
                    ("SpellID", "uint"),
                    ("SpellIconFileDataID", "signed"),
                ],
                &[
                    vec![U(3), U(123), I(999)],
                    vec![U(1), U(123), I(132337)],
                    vec![U(2), U(456), I(-1)],
                ],
            ),
        ]
    }
    pub(crate) fn fixture(rows: &[Vec<Cell>], spells: bool) -> Fixture {
        let mut tables = vec![sections(rows)];
        if spells {
            tables.extend(spell_tables());
        }
        Fixture::new(&tables)
    }
}
