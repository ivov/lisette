package dst_params

func Sum(dst []byte) int {
	total := 0
	for _, b := range dst {
		total += int(b)
	}
	return total
}

func Fill(dst []byte, v byte) {
	for i := range dst {
		dst[i] = v
	}
}
