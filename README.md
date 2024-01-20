bfstk, a brainfuck~ish interpreter

Implementation notes:

- the data pointer starts on cell zero;
- cells extend (virtually) unlimited to both the negative and positive directions; and
- no cell wraparound (i.e., underflow and overflow are fatal).

