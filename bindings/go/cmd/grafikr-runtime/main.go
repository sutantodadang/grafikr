package main

import (
	"flag"
	"fmt"
	"os"

	"github.com/grafikr/grafikr-go"
)

func main() {
	bundle := flag.String("bundle", "", "path to an unpacked Grafikr runtime bundle")
	cache := flag.String("cache", "", "directory for the verified runtime cache")
	flag.Parse()
	if *bundle == "" || *cache == "" {
		fmt.Fprintln(os.Stderr, "usage: grafikr-runtime --bundle <directory> --cache <directory>")
		os.Exit(2)
	}
	host, err := grafikr.InstallRuntime(*bundle, *cache)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Println(host)
}
