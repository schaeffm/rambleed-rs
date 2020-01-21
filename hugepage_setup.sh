#!/bin/bash
echo 512 > /sys/devices/system/node/node0/hugepages/hugepages-2048kB/nr_hugepages
echo 2 > /sys/devices/system/node/node0/hugepages/hugepages-1048576kB/nr_hugepages
