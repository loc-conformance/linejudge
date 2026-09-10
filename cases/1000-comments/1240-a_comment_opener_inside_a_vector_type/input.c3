<*
 Rotate every lane of the vector.
*>
macro rotate(int[<*>] x, uint shift)
{
	uint lanes = shift;
	return $$fshl(x, x, lanes);
}
