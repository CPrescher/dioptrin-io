#![cfg(feature = "wrappers")]
use dioptrin_io::{
    native, tiff,
    wrapped::{HeaderFormat, HeaderMetadata, HeaderReader, Limits},
};
#[test]
fn complete_headers_are_accessible_without_complete_pixels() {
    let header=b"{\nDim_1 = 3; Dim_2 = 2; DataType = UnsignedShort; ByteOrder = LowByteFirst; Size = 12; Motor = original;\n}\n";
    let r = HeaderReader::from_reader(
        &header[..],
        HeaderFormat::Edf,
        Limits::default(),
        native::Limits::default(),
        tiff::Limits::default(),
    )
    .unwrap();
    let HeaderMetadata::Edf(headers) = r.metadata().unwrap() else {
        panic!()
    };
    let images: Vec<_> = headers.images().collect();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].shape, [2, 3]);
    assert_eq!(images[0].fields["MOTOR"], "original");
    assert_eq!(images[0].header, header);
    assert!(
        HeaderReader::from_reader(
            &b"not an EDF"[..],
            HeaderFormat::Edf,
            Limits::default(),
            native::Limits::default(),
            tiff::Limits::default()
        )
        .is_err()
    );
}
#[test]
fn stack_local_and_general_headers_are_separate_original_records() {
    let general=b"{EDF_BinarySize=0; EDF_DataFormatVersion=1.0; Dim_1=2; Dim_2=1; DataType=UnsignedShort; ByteOrder=LowByteFirst; Motor=inherited;}\n";
    let image = b"{EDF_BinarySize=4; Motor=local;}\n";
    let mut bytes = general.to_vec();
    bytes.extend(image);
    bytes.extend([1, 0, 2, 0]);
    let r = HeaderReader::from_reader(
        &bytes[..],
        HeaderFormat::Edf,
        Limits::default(),
        native::Limits::default(),
        tiff::Limits::default(),
    )
    .unwrap();
    let HeaderMetadata::Edf(headers) = r.metadata().unwrap() else {
        panic!()
    };
    assert_eq!(headers.general_headers().len(), 1);
    assert_eq!(headers.general_headers()[0].fields["MOTOR"], "inherited");
    assert_eq!(headers.images().next().unwrap().fields["MOTOR"], "local");
}
