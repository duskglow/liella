// lgammaf's exception table (LIBM-LGAMMAF-EDGE-2), as written by `lgammaf_table_is_the_refused_set`
// (the exhaustive enumeration of every `float`; LIELLA_LGAMMAF_TABLE_OUT=<this file>): bits 32..63 x,
// 0..30 |F| for F the `float` nearest lnΓ(x), bit 31 set when |lnΓ(x)| lies above |F|; sorted by
// the bits of x.

const LGT: [u64; 18] = [
    0xc236ca27430270e4, // -4.5697414e1
    0xc26835374331b4c9, // -5.8051968e1
    0xc298aeecc3803702, // -7.6341644e1
    0xc33139a3c43991af, // -1.7722514e2
    0xc33807b8c441eead, // -1.8403015e2
    0xc36c073cc483b679, // -2.3602826e2
    0xc4af59534608f42e, // -1.4027914e3
    0xc6460e10c7d123ff, // -1.2675516e4
    0xc6975a8c4827d955, // -1.9373273e4
    0xc6e4d0da4884c94d, // -2.9288426e4
    0xc6f7e151c89116de, // -3.1728658e4
    0xc6fbee85c893b748, // -3.224726e4
    0xc7567ae3c904e3dc, // -5.4906887e4
    0xc76bc3c3c91378e1, // -6.035576e4
    0xc797bfc64942a14b, // -7.769555e4
    0xc8691f7fca25da6b, // -2.3871798e5
    0xc9aeb5a54b8fda19, // -1.4312206e6
    0xca46d773cc2df19c, // -3.2578208e6
];
