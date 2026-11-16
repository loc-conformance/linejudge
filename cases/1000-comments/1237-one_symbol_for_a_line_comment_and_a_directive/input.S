#include "riscv_test.h"
# the test number lives in gp
#define TESTNUM gp
#ifdef __riscv_compressed
# compressed instructions are allowed
#endif
    li TESTNUM, 2
