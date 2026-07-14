#![cfg(feature = "sd-storage")]

use binbook_fw::{
    owner_book::{
        can_reuse_warm_open, OwnerBookOpenClient, OwnerBookOpenUpdate, OwnerPageLoadClient,
        OwnerPageLoadUpdate, OwnerPlaneStream, OwnerPlaneStreamUpdate,
    },
    storage_owner::{Request, Response, ResponseEnvelope, StorageClient},
};

const BOOK: &[u8] =
    include_bytes!("../../../../crates/binbook-core/tests/fixtures/nav_probe.binbook");

fn response(sequence: u32, value: Response) -> ResponseEnvelope {
    ResponseEnvelope {
        client: StorageClient::Reader,
        sequence,
        response: Ok(value),
    }
}

#[test]
fn selected_book_opens_through_bounded_owner_reads_without_a_filesystem_borrow() {
    let mut opened = None;
    let mut buffer = [0u8; 8192];
    let (mut client, mut request) =
        OwnerBookOpenClient::begin("nav_probe.binbook", &mut opened, 50).unwrap();
    let mut open_reads = 0;
    loop {
        let update = match &request.request {
            Request::FileSize { name } => {
                assert_eq!(name.as_str(), "nav_probe.binbook");
                client
                    .accept(
                        response(request.sequence, Response::FileSize(BOOK.len() as u64)),
                        &mut buffer,
                    )
                    .unwrap()
            }
            Request::Read {
                name,
                offset,
                buffer_offset,
                len,
                ..
            } => {
                open_reads += 1;
                assert_eq!(name.as_str(), "nav_probe.binbook");
                assert!(usize::from(*len) <= 4096);
                let source = *offset as usize;
                let target = usize::from(*buffer_offset);
                buffer[target..target + usize::from(*len)]
                    .copy_from_slice(&BOOK[source..source + usize::from(*len)]);
                client
                    .accept(
                        response(request.sequence, Response::Read(*len)),
                        &mut buffer,
                    )
                    .unwrap()
            }
            _ => panic!("unexpected open request"),
        };
        match update {
            OwnerBookOpenUpdate::Request(next) => request = next,
            OwnerBookOpenUpdate::Complete => break,
        }
    }
    assert_eq!(
        open_reads, 1,
        "small book metadata should open from one prefix read"
    );
    drop(client);
    let opened = opened.unwrap();
    assert_eq!(opened.name().as_str(), "nav_probe.binbook");
    assert_eq!(opened.page_count(), 16);
    assert!(opened
        .rendition_identity()
        .rendition_hash
        .iter()
        .any(|byte| *byte != 0));
    assert_eq!(opened.index().page_count(), opened.page_count());

    let mut page = None;
    let (mut page_client, page_request) =
        OwnerPageLoadClient::begin(&opened, 5, &mut page, 500).unwrap();
    let Request::Read { offset, len, .. } = page_request.request else {
        panic!("page record read required")
    };
    buffer[..usize::from(len)]
        .copy_from_slice(&BOOK[offset as usize..offset as usize + usize::from(len)]);
    assert_eq!(
        page_client
            .accept(
                response(page_request.sequence, Response::Read(len)),
                &buffer,
            )
            .unwrap(),
        OwnerPageLoadUpdate::Complete
    );
    drop(page_client);
    let page = page.unwrap();
    assert_eq!(page.page_number.get(), 5);
    xteink_x4_display::profile::validate_page(&page).unwrap();

    let plane = page.planes.get(binbook_core::PlaneSlot::FastBase).unwrap();
    let (mut stream, mut plane_request) =
        OwnerPlaneStream::begin(&opened, plane, 800, 4096).unwrap();
    let mut rows = [0u8; 1600];
    let mut decoded = Vec::new();
    loop {
        let Request::Read { offset, len, .. } = plane_request.request else {
            panic!("plane input read required")
        };
        buffer[..usize::from(len)]
            .copy_from_slice(&BOOK[offset as usize..offset as usize + usize::from(len)]);
        let mut update = stream
            .accept(
                response(plane_request.sequence, Response::Read(len)),
                &buffer,
                &mut rows,
            )
            .unwrap();
        loop {
            match update {
                OwnerPlaneStreamUpdate::Request(next) => {
                    plane_request = next;
                    break;
                }
                OwnerPlaneStreamUpdate::RowsReady {
                    row_count,
                    complete,
                } => {
                    decoded.extend_from_slice(&rows[..usize::from(row_count) * 100]);
                    if complete {
                        let range = opened
                            .index()
                            .page_data_range(plane.offset.get(), plane.length.get())
                            .unwrap();
                        let compressed = &BOOK
                            [range.offset() as usize..range.offset() as usize + range.length()];
                        let mut expected = vec![0; 48_000];
                        binbook_decompress::decode_exact(
                            plane.compression,
                            compressed,
                            &mut expected,
                        )
                        .unwrap();
                        assert_eq!(decoded, expected);
                        return;
                    }
                    update = stream.advance(&buffer, &mut rows).unwrap();
                }
            }
        }
    }
}

#[test]
fn cached_book_open_activates_the_exact_mount_token_before_reading_metadata() {
    let mut opened = None;
    let token = binbook_fw::library::MountFileToken::new(73).unwrap();
    let (_, request) =
        OwnerBookOpenClient::begin_cached("selected.binbook", token, &mut opened, 900).unwrap();
    assert!(matches!(
        request.request,
        Request::ActivateFile { name, token: actual }
            if name.as_str() == "selected.binbook" && actual == token
    ));
}

#[test]
fn warm_open_requires_the_same_file_rendition_and_page() {
    let rendition = [7; 32];
    assert!(can_reuse_warm_open(11, 11, &rendition, &rendition, 6, 6));
    assert!(!can_reuse_warm_open(12, 11, &rendition, &rendition, 6, 6));
    assert!(!can_reuse_warm_open(11, 11, &[8; 32], &rendition, 6, 6));
    assert!(!can_reuse_warm_open(11, 11, &rendition, &rendition, 6, 5));
}
