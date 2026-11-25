#![expect(
    clippy::unwrap_used,
    reason = "build script must fail immediately on invalid extracted packet data"
)]

use rustc_hash::FxHashMap;
use std::{collections::BTreeMap, fs};

use heck::ToSnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Packets {
    version: i32,
    serverbound: BTreeMap<String, Vec<String>>,
    clientbound: BTreeMap<String, Vec<String>>,
}

/// Protocol numbers of the backward-compatible versions, oldest first, that the
/// reference `packets.json` (the current version) is translated against. Each has a
/// `packets-<protocol>.json` produced by `SteelExtractor` run against that version.
///
/// Keep in sync with `steel_protocol::version::MinecraftVersion`; a `steel-protocol`
/// test asserts the two lists agree so they cannot silently drift.
const LEGACY_PROTOCOLS: &[i32] = &[767, 768, 769, 770, 771, 772, 773, 774, 775, 776];

/// Marker for "this version has no such packet" in an id matrix row.
const ABSENT: i16 = -1;

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/packets.json");

    let packets: Packets =
        serde_json::from_str(&fs::read_to_string("build_assets/packets.json").unwrap())
            .expect("Failed to parse packets.json");

    let mut phases: FxHashMap<String, TokenStream> = FxHashMap::default();

    let version = packets.version;
    parse_packets(
        &packets.clientbound,
        Ident::new("C", Span::call_site()),
        &mut phases,
    );
    parse_packets(
        &packets.serverbound,
        Ident::new("S", Span::call_site()),
        &mut phases,
    );

    let consts: TokenStream = phases
        .iter()
        .map(|(p, entries)| {
            let phase = Ident::new(p, Span::call_site());
            quote! {
                pub mod #phase {
                    #entries
                }
            }
        })
        .collect();

    let legacy = build_legacy(&packets, version);
    quote!(
        /// The current Minecraft protocol version. This changes only when the protocol itself is modified.
        pub const CURRENT_MC_PROTOCOL: i32 = #version;

        #consts

        #legacy
    )
}

pub(crate) fn parse_packets(
    packets: &BTreeMap<String, Vec<String>>,
    prefix: Ident,
    phases: &mut FxHashMap<String, TokenStream>,
) {
    for packet in packets {
        let inner = phases.entry(packet.0.to_snake_case()).or_default();

        for (id, packet_name) in packet.1.iter().enumerate() {
            let packet_id = id as i32;
            let packet_name = packet_name.replace('/', "_");
            let name = format!("{prefix}_{packet_name}").to_uppercase();
            let name = format_ident!("{}", name);
            inner.extend([quote! {
                pub const #name: i32 = #packet_id;
            }]);
        }
    }
}

/// Loads every available legacy packet table, oldest first, keyed by protocol number.
///
/// A missing `packets-<protocol>.json` is skipped rather than failed: the server keeps
/// supporting only the versions whose extracted data is present, so an unextracted
/// version is never advertised. This keeps the feature from shipping with fabricated ids.
fn load_legacy_tables() -> Vec<(i32, Packets)> {
    let mut tables = Vec::new();
    for &protocol in LEGACY_PROTOCOLS {
        let path = format!("build_assets/packets-{protocol}.json");
        println!("cargo:rerun-if-changed={path}");
        let Ok(raw) = fs::read_to_string(&path) else {
            println!(
                "cargo:warning=No packet table for protocol {protocol}; version not advertised"
            );
            continue;
        };
        let parsed: Packets =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("Failed to parse {path}: {e}"));
        assert_eq!(
            parsed.version, protocol,
            "{path} declares version {}, expected {protocol}",
            parsed.version
        );
        tables.push((protocol, parsed));
    }
    tables.sort_by_key(|(protocol, _)| *protocol);
    tables
}

/// Builds the `legacy` module: per direction/phase remap tables indexed by version column.
///
/// Clientbound tables map a current-version id to that version's id (`-1` when the packet
/// does not exist there); serverbound tables map a client id back to the current id. The
/// last column is always the current version and is the identity map. Packets are matched
/// across versions by their extracted `PacketType` path, so no id is transcribed by hand.
fn build_legacy(current: &Packets, current_protocol: i32) -> TokenStream {
    let tables = load_legacy_tables();
    let protocols: Vec<i32> = tables
        .iter()
        .map(|(protocol, _)| *protocol)
        .chain([current_protocol])
        .collect();

    let protocols_lit = protocols.iter();
    let version_count = protocols.len();
    let absent = ABSENT;

    let clientbound = build_direction(&tables, &current.clientbound, true);
    let serverbound = build_direction(&tables, &current.serverbound, false);

    quote! {
        /// Packet-id translation tables for backward-compatible client versions.
        ///
        /// Generated from `packets.json` (the current version) plus one
        /// `packets-<protocol>.json` per extracted older version. Packets are aligned by
        /// their registry path, never by a hand-written id.
        pub mod legacy {
            /// Protocol numbers with an available table, oldest first; the last is current.
            pub const VERSION_PROTOCOLS: &[i32] = &[#(#protocols_lit),*];

            /// The current version's protocol number (the last column).
            pub const CURRENT_PROTOCOL: i32 = #current_protocol;

            /// A row value meaning "this version has no such packet"; drop the packet.
            pub const ABSENT: i16 = #absent;

            /// Column index of `protocol`, or `None` when no table covers it.
            #[must_use]
            pub const fn column_of(protocol: i32) -> Option<usize> {
                let mut index = 0;
                while index < VERSION_PROTOCOLS.len() {
                    if VERSION_PROTOCOLS[index] == protocol {
                        return Some(index);
                    }
                    index += 1;
                }
                None
            }

            /// Number of version columns in every table.
            pub const REMAP_WIDTH: usize = #version_count;

            /// Phase indices into [`CLIENTBOUND_REMAPS`] and [`SERVERBOUND_REMAPS`].
            ///
            /// A direction without one of these phases gets an empty table, so a lookup
            /// there yields no mapping instead of indexing the wrong phase.
            pub const PHASE_HANDSHAKE: usize = 0;
            pub const PHASE_STATUS: usize = 1;
            pub const PHASE_LOGIN: usize = 2;
            pub const PHASE_CONFIG: usize = 3;
            pub const PHASE_PLAY: usize = 4;

            /// `CLIENTBOUND_REMAPS[phase][column][current_id] = client_id`.
            ///
            /// Clientbound tables map a current-version id to that version's id, or
            /// [`ABSENT`] when the version does not have the packet.
            pub static CLIENTBOUND_REMAPS: &[&[&[i16]]] = &[#clientbound];

            /// `SERVERBOUND_REMAPS[phase][column][client_id] = current_id`.
            ///
            /// Serverbound tables map a client id back to the current id, or [`ABSENT`]
            /// when that client id is unknown to the version.
            pub static SERVERBOUND_REMAPS: &[&[&[i16]]] = &[#serverbound];
        }
    }
}

/// Emits one comma-separated `&[..]` entry per phase slot, in [`PHASE_*`] order.
///
/// Phases the direction does not use (clientbound has no handshake) emit an empty table.
fn build_direction(
    tables: &[(i32, Packets)],
    current_phase_map: &BTreeMap<String, Vec<String>>,
    clientbound: bool,
) -> TokenStream {
    let slots = ["handshake", "status", "login", "config", "play"];
    let entries: Vec<TokenStream> = slots
        .iter()
        .map(|phase| {
            let Some(current_names) = current_phase_map.get(*phase) else {
                return quote! { &[] };
            };

            // One column per extracted legacy version, then the current identity column.
            let mut columns: Vec<Vec<i16>> = Vec::new();
            for (protocol, versioned) in tables {
                let version_names = phase_names(versioned, phase, clientbound, *protocol);
                columns.push(remap_column(current_names, &version_names, clientbound));
            }
            columns.push(identity_column(current_names.len()));

            let column_lits = columns.iter().map(|column| {
                let ids = column.iter();
                quote! { &[#(#ids),*] }
            });
            quote! { &[#(#column_lits),*] }
        })
        .collect();

    quote! { #(#entries),* }
}

/// The `from -> to` row for one version column.
///
/// Clientbound: iterate current packets, look up each one's id in the older version by
/// name. Serverbound: index by the older version's id, look up the current id by name.
fn remap_column(current_names: &[String], version_names: &[String], clientbound: bool) -> Vec<i16> {
    if clientbound {
        current_names
            .iter()
            .map(|name| position(version_names, name).map_or(ABSENT, |id| id as i16))
            .collect()
    } else {
        version_names
            .iter()
            .map(|name| position(current_names, name).map_or(ABSENT, |id| id as i16))
            .collect()
    }
}

/// The current version's identity column: every id maps to itself.
fn identity_column(len: usize) -> Vec<i16> {
    (0..len).map(|id| id as i16).collect()
}

fn phase_names(versioned: &Packets, phase: &str, clientbound: bool, protocol: i32) -> Vec<String> {
    let map = if clientbound {
        &versioned.clientbound
    } else {
        &versioned.serverbound
    };
    map.get(phase)
        .unwrap_or_else(|| panic!("packets-{protocol}.json has no {phase} phase"))
        .clone()
}

fn position(names: &[String], name: &str) -> Option<usize> {
    names.iter().position(|candidate| candidate == name)
}
