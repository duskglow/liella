/* Spec §1.2 input clause 1: only the low 8 bits of the status reach the parent. main's return
 * value is passed to exit. */
/* EXPECT-EXIT: 7 */
int main(void)
{
    return 256 + 7;
}
