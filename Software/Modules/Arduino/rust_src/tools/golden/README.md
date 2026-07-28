# Golden-frame generator

`crates/gowired-core/src/tests/message.rs` asserts that this firmware's encoder
produces the same bytes as the MySensors C++ library. Those bytes were not
derived from the specification -- they were produced by compiling the library's
own `MyMessage.cpp` on the host and dumping its memory.

That matters, because "I read the header and implemented what it says" and
"I produce the same bytes the deployed firmware produced" are different claims,
and only the second one is worth anything to a controller that is already paired.

## Reproducing

```sh
cd tools/golden
g++ -std=gnu++11 -I. -I ~/Arduino/libraries/MySensors -I ~/Arduino/libraries/MySensors/core \
    gen.cpp ~/Arduino/libraries/MySensors/core/MyMessage.cpp -o gen
./gen
```

The output is Rust source: paste it over the `GOLDEN` table in
`crates/gowired-core/src/tests/message.rs`.

`Arduino.h` here is a stand-in with just enough in it for `MyMessage.cpp` to
compile away from the AVR toolchain: `dtostrf`, the `itoa` family, and the three
address constants. None of it touches the wire format -- `MyMessage` is a packed
struct that `dump()` reads byte by byte, so what comes out is exactly what the
library would have handed to the transport.

Tested against MySensors 2.4.0-rc.1, the version the C++ firmware used.
`sizeof(MyMessage)` is 33: a 7-byte header, 25 bytes of payload and the extra
NUL the library keeps for printing (never transmitted).
