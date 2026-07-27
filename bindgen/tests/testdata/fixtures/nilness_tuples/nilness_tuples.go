// Fixture: non-error tuples. A witnessed nil element demotes to Option, the
// rest stay bare.
package nilness_tuples

type Block struct{ Data []byte }

func Decode(data []byte) (*Block, []byte) {
	if len(data) == 0 {
		return nil, data
	}
	return &Block{Data: data}, nil
}

func Split(data []byte) (*Block, *Block) {
	half := len(data) / 2
	return &Block{Data: data[:half]}, &Block{Data: data[half:]}
}

func DecodeForwarded(data []byte) (*Block, []byte) {
	return Decode(data)
}
