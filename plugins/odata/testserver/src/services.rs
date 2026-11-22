// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The four synthetic services, described by hand: what the server needs of each `$metadata`
//! (keys, ETag property, navigation, capabilities, drafts). The EDMX files are served as they
//! are; this table must agree with them (`tests/server.rs` checks the names).

/// OData protocol version of a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    V2,
    V4,
}

/// Whose behaviour a service imitates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// SAP Gateway / RAP on S/4HANA: Basic auth, `sap-client`, `If-Match` required.
    S4,
    /// CAP: no auth, `If-Match` optional but checked when sent.
    Cap,
}

/// Which draft actions a V4 service offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftStyle {
    /// `Edit`, `Prepare`, `Activate`, `Discard` (bound actions).
    Rap,
    /// `draftEdit`, `draftPrepare`, `draftActivate`; discard is `DELETE` on the draft.
    Cap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    Str,
    Int,
    Guid,
    Bool,
}

/// How a missing key is filled on create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Generate {
    /// A 10-digit number one above the largest existing one (`0500000004`).
    Number10,
    /// The next multiple of 10 among the siblings with the same first key, 6 digits.
    ItemPosition,
    /// A new GUID.
    Guid,
    /// One above the largest integer.
    NextInt,
}

/// How the ETag property is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stamp {
    /// `2026-11-22T10:00:01.0000000Z` (S/4).
    Seven,
    /// `2026-11-22T10:00:01.000Z` (CAP).
    Milli,
}

pub struct NavDef {
    pub name: &'static str,
    pub target: &'static str,
    pub many: bool,
    /// `(property of this entity, property of the target)` pairs that must be equal.
    pub on: &'static [(&'static str, &'static str)],
}

pub struct SetDef {
    pub name: &'static str,
    /// Key in the service's `data.json`.
    pub data: &'static str,
    /// Qualified entity type.
    pub entity_type: &'static str,
    pub keys: &'static [(&'static str, KeyKind)],
    pub etag: Option<(&'static str, Stamp)>,
    pub generate: &'static [(&'static str, Generate)],
    pub navs: &'static [NavDef],
    pub insertable: bool,
    pub updatable: bool,
    pub deletable: bool,
    pub draft_root: bool,
    /// Properties set by the server on create and update (`Core.Computed`).
    pub computed: &'static [&'static str],
}

pub struct ServiceDef {
    pub root: &'static str,
    pub edmx: &'static str,
    pub version: Version,
    pub flavor: Flavor,
    pub draft: Option<DraftStyle>,
    pub namespace: &'static str,
    /// The service's `data.json`.
    pub data: &'static str,
    pub sets: &'static [SetDef],
}

impl ServiceDef {
    pub fn set(&self, name: &str) -> Option<&SetDef> {
        self.sets.iter().find(|s| s.name == name)
    }
}

impl SetDef {
    pub fn nav(&self, name: &str) -> Option<&NavDef> {
        self.navs.iter().find(|n| n.name == name)
    }

    pub fn has_draft_keys(&self) -> bool {
        self.keys.iter().any(|(k, _)| *k == "IsActiveEntity")
    }
}

const S4_DATA: &str = include_str!("../../testdata/s4/data.json");
const CAP_DATA: &str = include_str!("../../testdata/cap-bookshop/data.json");

pub static SERVICES: &[ServiceDef] = &[
    ServiceDef {
        root: "/sap/opu/odata/sap/ZRT_SALES_SRV/",
        edmx: include_str!("../../testdata/s4/ZRT_SALES_SRV.edmx"),
        version: Version::V2,
        flavor: Flavor::S4,
        draft: None,
        namespace: "ZRT_SALES_SRV",
        data: S4_DATA,
        sets: &[
            SetDef {
                name: "SalesOrderSet",
                data: "SalesOrderSet",
                entity_type: "ZRT_SALES_SRV.SalesOrder",
                keys: &[("SalesOrderID", KeyKind::Str)],
                etag: Some(("ChangedAt", Stamp::Seven)),
                generate: &[("SalesOrderID", Generate::Number10)],
                navs: &[
                    NavDef {
                        name: "ToItems",
                        target: "SalesOrderItemSet",
                        many: true,
                        on: &[("SalesOrderID", "SalesOrderID")],
                    },
                    NavDef {
                        name: "ToCustomer",
                        target: "CustomerSet",
                        many: false,
                        on: &[("CustomerID", "CustomerID")],
                    },
                ],
                insertable: true,
                updatable: true,
                deletable: false,
                draft_root: false,
                computed: &["CreatedAt", "ChangedAt"],
            },
            SetDef {
                name: "SalesOrderItemSet",
                data: "SalesOrderItemSet",
                entity_type: "ZRT_SALES_SRV.SalesOrderItem",
                keys: &[
                    ("SalesOrderID", KeyKind::Str),
                    ("ItemPosition", KeyKind::Str),
                ],
                etag: None,
                generate: &[("ItemPosition", Generate::ItemPosition)],
                navs: &[NavDef {
                    name: "ToSalesOrder",
                    target: "SalesOrderSet",
                    many: false,
                    on: &[("SalesOrderID", "SalesOrderID")],
                }],
                insertable: true,
                updatable: true,
                deletable: true,
                draft_root: false,
                computed: &[],
            },
            SetDef {
                name: "CustomerSet",
                data: "CustomerSet",
                entity_type: "ZRT_SALES_SRV.Customer",
                keys: &[("CustomerID", KeyKind::Str)],
                etag: None,
                generate: &[],
                navs: &[],
                insertable: false,
                updatable: false,
                deletable: false,
                draft_root: false,
                computed: &[],
            },
        ],
    },
    ServiceDef {
        root: "/sap/opu/odata4/sap/zrt_salesorder/srvd_a2x/sap/zrt_salesorder/0001/",
        edmx: include_str!("../../testdata/s4/zrt_salesorder_0001.edmx"),
        version: Version::V4,
        flavor: Flavor::S4,
        draft: Some(DraftStyle::Rap),
        namespace: "com.example.zrt_salesorder",
        data: S4_DATA,
        sets: &[
            SetDef {
                name: "SalesOrder",
                data: "SalesOrder",
                entity_type: "com.example.zrt_salesorder.SalesOrderType",
                keys: &[
                    ("SalesOrderUUID", KeyKind::Guid),
                    ("IsActiveEntity", KeyKind::Bool),
                ],
                etag: Some(("LastChangedAt", Stamp::Seven)),
                generate: &[
                    ("SalesOrderUUID", Generate::Guid),
                    ("SalesOrderID", Generate::Number10),
                ],
                navs: &[NavDef {
                    name: "_Item",
                    target: "SalesOrderItem",
                    many: true,
                    on: &[("SalesOrderUUID", "SalesOrderUUID")],
                }],
                insertable: true,
                updatable: true,
                deletable: false,
                draft_root: true,
                computed: &["SalesOrderID", "LastChangedAt"],
            },
            SetDef {
                name: "SalesOrderItem",
                data: "SalesOrderItem",
                entity_type: "com.example.zrt_salesorder.SalesOrderItemType",
                keys: &[
                    ("ItemUUID", KeyKind::Guid),
                    ("IsActiveEntity", KeyKind::Bool),
                ],
                etag: None,
                generate: &[("ItemUUID", Generate::Guid)],
                navs: &[NavDef {
                    name: "_SalesOrder",
                    target: "SalesOrder",
                    many: false,
                    on: &[("SalesOrderUUID", "SalesOrderUUID")],
                }],
                insertable: true,
                updatable: true,
                deletable: true,
                draft_root: false,
                computed: &[],
            },
        ],
    },
    ServiceDef {
        root: "/odata/v4/admin/",
        edmx: include_str!("../../testdata/cap-bookshop/admin-v4.edmx"),
        version: Version::V4,
        flavor: Flavor::Cap,
        draft: Some(DraftStyle::Cap),
        namespace: "AdminService",
        data: CAP_DATA,
        sets: &[
            SetDef {
                name: "Books",
                data: "Books",
                entity_type: "AdminService.Books",
                keys: &[("ID", KeyKind::Int), ("IsActiveEntity", KeyKind::Bool)],
                etag: Some(("modifiedAt", Stamp::Milli)),
                generate: &[("ID", Generate::NextInt)],
                navs: &[
                    NavDef {
                        name: "author",
                        target: "Authors",
                        many: false,
                        on: &[("author_ID", "ID")],
                    },
                    NavDef {
                        name: "genre",
                        target: "Genres",
                        many: false,
                        on: &[("genre_ID", "ID")],
                    },
                ],
                insertable: true,
                updatable: true,
                deletable: true,
                draft_root: true,
                computed: &["createdAt", "modifiedAt"],
            },
            SetDef {
                name: "Authors",
                data: "Authors",
                entity_type: "AdminService.Authors",
                keys: &[("ID", KeyKind::Int)],
                etag: None,
                generate: &[("ID", Generate::NextInt)],
                navs: &[NavDef {
                    name: "books",
                    target: "Books",
                    many: true,
                    on: &[("ID", "author_ID")],
                }],
                insertable: true,
                updatable: true,
                deletable: true,
                draft_root: false,
                computed: &[],
            },
            SetDef {
                name: "Genres",
                data: "Genres",
                entity_type: "AdminService.Genres",
                keys: &[("ID", KeyKind::Int)],
                etag: None,
                generate: &[("ID", Generate::NextInt)],
                navs: &[],
                insertable: true,
                updatable: true,
                deletable: true,
                draft_root: false,
                computed: &[],
            },
        ],
    },
    ServiceDef {
        root: "/odata/v2/admin/",
        edmx: include_str!("../../testdata/cap-bookshop/admin-v2.edmx"),
        version: Version::V2,
        flavor: Flavor::Cap,
        draft: None,
        namespace: "AdminService",
        data: CAP_DATA,
        sets: &[
            SetDef {
                name: "Books",
                data: "Books",
                entity_type: "AdminService.Books",
                keys: &[("ID", KeyKind::Int)],
                etag: Some(("modifiedAt", Stamp::Milli)),
                generate: &[("ID", Generate::NextInt)],
                navs: &[NavDef {
                    name: "author",
                    target: "Authors",
                    many: false,
                    on: &[("author_ID", "ID")],
                }],
                insertable: true,
                updatable: true,
                deletable: true,
                draft_root: false,
                computed: &["createdAt", "modifiedAt"],
            },
            SetDef {
                name: "Authors",
                data: "Authors",
                entity_type: "AdminService.Authors",
                keys: &[("ID", KeyKind::Int)],
                etag: None,
                generate: &[("ID", Generate::NextInt)],
                navs: &[NavDef {
                    name: "books",
                    target: "Books",
                    many: true,
                    on: &[("ID", "author_ID")],
                }],
                insertable: true,
                updatable: true,
                deletable: true,
                draft_root: false,
                computed: &[],
            },
            SetDef {
                name: "Genres",
                data: "Genres",
                entity_type: "AdminService.Genres",
                keys: &[("ID", KeyKind::Int)],
                etag: None,
                generate: &[("ID", Generate::NextInt)],
                navs: &[],
                insertable: true,
                updatable: true,
                deletable: true,
                draft_root: false,
                computed: &[],
            },
        ],
    },
];
