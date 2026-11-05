//! Bounded adaptation of dxtbx's BSD-3-Clause TY5 mode5 decoder.
//! Authority: cctbx/dxtbx 41fc3d3cfd26c12e8cb3595b68360a9a9bdbcb05,
//! src/dxtbx/boost_python/compression.cc:rod_TY5_TY6_decompress and
//! src/dxtbx/format/FormatROD.py:_get_raw_data_ty5_ty6_native.
//! Copyright (c) 2019 Diamond Light Source, Lawrence Berkeley National
//! Laboratory and the Science and Technology Facilities Council.
//! Retained license: licenses/native-readers/dxtbx-TY5-BSD-3-Clause.txt.
use super::Error;

pub(super) fn decode(
    input: &[u8],
    shape: [usize; 2],
    exceptions: [usize; 2],
    mut emit: impl FnMut(usize, i32) -> Result<(), Error>,
) -> Result<(), Error> {
    let [height, width] = shape;
    let table_bytes = height
        .checked_mul(4)
        .ok_or_else(|| Error("Oxford TY5 row table size overflow".into()))?;
    let data_len = input
        .len()
        .checked_sub(table_bytes)
        .ok_or_else(|| Error("truncated Oxford TY5 row table".into()))?;
    let (data, table) = input.split_at(data_len);
    let mut expected = 0usize;
    let mut actual = [0usize; 2];
    for row in 0..height {
        let start = u32::from_le_bytes(table[row * 4..row * 4 + 4].try_into().unwrap()) as usize;
        let end = if row + 1 == height {
            data_len
        } else {
            u32::from_le_bytes(table[row * 4 + 4..row * 4 + 8].try_into().unwrap()) as usize
        };
        if start != expected || end <= start || end > data_len {
            return Err(Error(
                "Oxford TY5 row offsets overlap, skip data or exceed payload".into(),
            ));
        }
        let mut cursor = start;
        let mut current = 0i32;
        for column in 0..width {
            let marker = *data
                .get(cursor)
                .filter(|_| cursor < end)
                .ok_or_else(|| Error("truncated Oxford TY5 row bytecode".into()))?;
            cursor += 1;
            let delta = match marker {
                254 => {
                    actual[0] += 1;
                    i32::from(i16::from_le_bytes(take(data, &mut cursor, end)?))
                }
                255 => {
                    actual[1] += 1;
                    i32::from_le_bytes(take(data, &mut cursor, end)?)
                }
                value => i32::from(value) - 127,
            };
            // Each row's first stored value is absolute; following codes are
            // signed differences. Reject overflow instead of C++ signed UB.
            current = current
                .checked_add(delta)
                .ok_or_else(|| Error("Oxford TY5 signed32 cumulative overflow".into()))?;
            emit(row * width + column, current)?;
        }
        if cursor != end {
            return Err(Error(
                "Oxford TY5 row contains unexplained encoded bytes".into(),
            ));
        }
        expected = end;
    }
    if expected != data_len || actual != exceptions {
        return Err(Error(
            "Oxford TY5 exception counts/payload length disagree".into(),
        ));
    }
    Ok(())
}
fn take<const N: usize>(data: &[u8], cursor: &mut usize, end: usize) -> Result<[u8; N], Error> {
    let stop = cursor
        .checked_add(N)
        .filter(|&n| n <= end)
        .ok_or_else(|| Error("truncated Oxford TY5 inline exception".into()))?;
    let value = data
        .get(*cursor..stop)
        .ok_or_else(|| Error("truncated Oxford TY5 inline exception".into()))?;
    *cursor = stop;
    Ok(value.try_into().unwrap())
}
