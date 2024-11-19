use parabuild::get_cuda_device_uuids;

fn main(){
    let cuda_device_uuids = get_cuda_device_uuids();
    println!("{:?}", cuda_device_uuids);
}