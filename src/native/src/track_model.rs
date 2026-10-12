use std::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QHash, QModelIndex, QString, QVariant, QVector};
use serde::Deserialize;

#[cxx_qt::bridge(namespace = "Qplay")]
pub mod ffi {
    #[namespace = ""]
    unsafe extern "C++" {
        include!(<QAbstractListModel>);
        type QAbstractListModel;
        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
        include!("cxx-qt-lib/qvector.h");
        type QVector_i32 = cxx_qt_lib::QVector<i32>;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_uncreatable]
        #[base = QAbstractListModel]
        type TrackModel = super::TrackModelRust;

        #[cxx_override]
        #[rust_name = "row_count"]
        fn rowCount(&self, parent: &QModelIndex) -> i32;
        #[cxx_override]
        fn data(&self, index: &QModelIndex, role: i32) -> QVariant;
        #[cxx_override]
        #[rust_name = "role_names"]
        fn roleNames(&self) -> QHash_i32_QByteArray;

        #[inherit]
        #[rust_name = "begin_reset_model"]
        fn beginResetModel(self: Pin<&mut Self>);
        #[inherit]
        #[rust_name = "end_reset_model"]
        fn endResetModel(self: Pin<&mut Self>);
        #[inherit]
        fn index(&self, row: i32, column: i32, parent: &QModelIndex) -> QModelIndex;
        #[inherit]
        #[qsignal]
        #[rust_name = "data_changed"]
        fn dataChanged(
            self: Pin<&mut Self>,
            first: &QModelIndex,
            last: &QModelIndex,
            roles: &QVector_i32,
        );
    }

    unsafe extern "C++" {
        include!("qplay_native/native.h");
        #[rust_name = "create_track_model"]
        fn createTrackModel() -> UniquePtr<TrackModel>;
    }
}

#[derive(Clone, Deserialize, PartialEq)]
pub struct Track {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub selected: bool,
}

impl Track {
    fn label(&self) -> String {
        if !self.title.is_empty() {
            return self.title.clone();
        }
        if self.lang == "und" || self.lang == "unk" {
            String::new()
        } else {
            self.lang.clone()
        }
    }
}

#[derive(Default)]
pub struct TrackModelRust {
    pub rows: Vec<Track>,
}

impl ffi::TrackModel {
    fn row_count(&self, parent: &QModelIndex) -> i32 {
        if parent.is_valid() {
            0
        } else {
            self.rows.len() as i32
        }
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        if !index.is_valid() || index.column() != 0 {
            return QVariant::default();
        }
        let Some(track) = self.rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        match role - 256 {
            0 => QVariant::from(&track.id),
            1 => QVariant::from(&QString::from(track.kind.as_str())),
            2 => QVariant::from(&QString::from(track.label())),
            3 => QVariant::from(&track.selected),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> QHash<cxx_qt_lib::QHashPair_i32_QByteArray> {
        let mut roles = QHash::default();
        for (index, name) in ["trackId", "kind", "label", "selected"]
            .into_iter()
            .enumerate()
        {
            roles.insert(256 + index as i32, QByteArray::from(name));
        }
        roles
    }

    pub fn update(mut self: Pin<&mut Self>, rows: Vec<Track>) {
        if self.rows == rows {
            return;
        }
        let same_tracks = self.rows.len() == rows.len()
            && self
                .rows
                .iter()
                .zip(&rows)
                .all(|(left, right)| left.id == right.id && left.kind == right.kind);
        if same_tracks {
            self.as_mut().rust_mut().rows = rows;
            if !self.rows.is_empty() {
                let parent = QModelIndex::default();
                let first = self.index(0, 0, &parent);
                let last = self.index(self.rows.len() as i32 - 1, 0, &parent);
                self.as_mut()
                    .data_changed(&first, &last, &QVector::default());
            }
        } else {
            self.as_mut().begin_reset_model();
            self.as_mut().rust_mut().rows = rows;
            self.as_mut().end_reset_model();
        }
    }
}
